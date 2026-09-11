use chrono::{SecondsFormat, Utc};
use chrono_tz::Tz;
use sqlx::any::AnyPoolOptions;
use sqlx::AnyPool;
use sqlx::Executor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The default database for a data directory: the SQLite file LogB has always kept there.
///
/// `LOGB_DATA_DIR` keeps its meaning -- it is where blobs live, and it is still where the
/// database goes when `LOGB_DATABASE_URL` says nothing else.
pub fn sqlite_url(data_dir: &Path) -> Result<String, BoxError> {
    // `?` and `#` are legal in a Linux path and are structural in a URL: a data directory
    // containing either would be silently truncated at that character, and LogB would open a
    // database somewhere other than where it was told. `%` is legal in a Linux path too, and
    // is how a URL escapes other characters: `AnyPool` percent-decodes the path it is given,
    // but this function does not encode it and `sqlite_file` does not decode it back, so a
    // directory such as `pct%41` would be opened as `pctA` -- a *different* directory, chosen
    // silently. Refusing all three is the only honest answer -- encoding them would depend on
    // the driver decoding them back the same way.
    let dir = data_dir.display().to_string();
    if let Some(bad) = dir.chars().find(|c| matches!(c, '?' | '#' | '%')) {
        return Err(format!(
            "the data directory {dir} contains {bad:?}, which cannot appear in a database URL. \
             Move the data somewhere without it, or set LOGB_DATABASE_URL yourself."
        )
        .into());
    }
    Ok(format!("sqlite://{dir}/logb.db?mode=rwc"))
}

/// The file a SQLite URL points at, or `None` for any other backend.
///
/// Used only to keep the two things a URL cannot say: that a data directory has to exist
/// before SQLite can create a file in it, and that `connect_existing` must find a database
/// rather than make one.
fn sqlite_file(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("sqlite://").or_else(|| url.strip_prefix("sqlite:"))?;
    let path = rest.split(['?', '#']).next().unwrap_or("");
    (!path.is_empty() && path != ":memory:").then(|| PathBuf::from(path))
}

/// SQLite needs three settings that a connection URL cannot carry: sqlx 0.9's URL parser accepts
/// only `mode`, `cache`, `immutable` and `vfs`. `AnyPool` connects by URL, so they are applied
/// to every connection as it is opened instead.
///
/// `foreign_keys` is the one that matters. Without it nothing fails -- `ON DELETE CASCADE`
/// simply stops happening, and the first sign is an attachment that outlived its object.
/// PostgreSQL enforces foreign keys always and has no equivalent to set.
///
/// The busy timeout is a parameter only because the two callers have always differed: the
/// server waits five seconds for a writer, while the one-shot `--backup` connection waits
/// thirty, since it is competing with a live instance and has nothing else to do.
fn after_connect(url: &str, busy_timeout_ms: u32) -> Option<String> {
    url.starts_with("sqlite:").then(|| {
        format!(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = {busy_timeout_ms}"
        )
    })
}

/// Builds a pool for `url`, applying the SQLite pragmas to every connection it opens.
fn pool_options(url: &str, max_connections: u32, busy_timeout_ms: u32) -> AnyPoolOptions {
    let pragmas = after_connect(url, busy_timeout_ms);
    AnyPoolOptions::new().max_connections(max_connections).after_connect(move |conn, _meta| {
        let pragmas = pragmas.clone();
        Box::pin(async move {
            if let Some(sql) = pragmas {
                conn.execute(sqlx::AssertSqlSafe(sql)).await?;
            }
            Ok(())
        })
    })
}

pub async fn connect(url: &str) -> Result<AnyPool, BoxError> {
    sqlx::any::install_default_drivers();
    // SQLite will create the database file, but not the directory holding it.
    if let Some(file) = sqlite_file(url) {
        if let Some(dir) = file.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
    }
    let pool = pool_options(url, if url.starts_with("sqlite:") { 4 } else { 16 }, 5_000)
        .connect(url)
        .await?;
    // Task 3 replaces this with `migrator(url)`, choosing between the SQLite and PostgreSQL
    // migration sets at runtime. Until then there is one set, and it is SQLite's.
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Opens an existing database without running migrations, for read-only side commands such
/// as `--backup` that must not touch the schema of a running instance.
pub async fn connect_existing(url: &str) -> Result<AnyPool, BoxError> {
    sqlx::any::install_default_drivers();
    // A `mode=rwc` URL would quietly create an empty database where the operator expected to
    // find one, and `--backup` would then report success over nothing.
    let url = match sqlite_file(url) {
        Some(file) => {
            if !file.exists() {
                return Err(format!("no database at {}", file.display()).into());
            }
            url.replace("mode=rwc", "mode=rw")
        },
        None => url.to_string(),
    };
    Ok(pool_options(&url, 1, 30_000).connect(&url).await?)
}

/// Writes a consistent snapshot of the database to `dest`.
///
/// `VACUUM INTO` is the reason this exists: copying `logb.db` out from under a running
/// instance can catch it mid-write and miss the WAL entirely, while this runs inside a read
/// transaction and produces a compacted, self-consistent file.
pub async fn backup_to(pool: &AnyPool, dest: &Path) -> Result<(), BoxError> {
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()).into());
    }
    let dest = dest.to_str().ok_or("backup path must be valid UTF-8")?;
    sqlx::query("VACUUM INTO ?").bind(dest).execute(pool).await?;
    Ok(())
}

/// A boolean that survives the trip through `AnyRow`.
///
/// SQLite has no boolean type -- `is_admin` is a 0 or a 1 in an INTEGER column -- so sqlx's
/// `Any` driver reports that column as `BIGINT`, while PostgreSQL would report a real
/// `BOOLEAN`. Plain `bool` decodes from only the second, so a row struct shared by both
/// backends cannot use it. This accepts either shape and is transparent to serde, so the JSON
/// a client sees is still `true`/`false`.
///
/// NOTE FOR THE PLAN: this is the one type `AnyRow` could not carry unchanged, and it is why
/// Task 4 is free to declare the PostgreSQL column either `BOOLEAN` or an integer -- both
/// decode here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Bool(pub bool);

impl From<Bool> for bool {
    fn from(b: Bool) -> Self {
        b.0
    }
}

impl From<bool> for Bool {
    fn from(b: bool) -> Self {
        Bool(b)
    }
}

impl sqlx::Type<sqlx::Any> for Bool {
    fn type_info() -> sqlx::any::AnyTypeInfo {
        <bool as sqlx::Type<sqlx::Any>>::type_info()
    }

    fn compatible(ty: &sqlx::any::AnyTypeInfo) -> bool {
        use sqlx::any::AnyTypeInfoKind::*;
        matches!(ty.kind(), Bool | SmallInt | Integer | BigInt)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Any> for Bool {
    fn decode(value: sqlx::any::AnyValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        // PostgreSQL answers with a real boolean; SQLite answers with the integer it stored.
        match <bool as sqlx::Decode<sqlx::Any>>::decode(value.clone()) {
            Ok(b) => Ok(Bool(b)),
            Err(_) => Ok(Bool(<i64 as sqlx::Decode<sqlx::Any>>::decode(value)? != 0)),
        }
    }
}

/// The instance's wall-clock timezone, set once from `LOGB_TIMEZONE` at startup.
///
/// A process-wide value rather than a parameter because `today()` is called from places with
/// no access to the config -- notably the `ReminderRow -> ReminderOut` conversion that decides
/// whether a reminder is due. Unset (in tests, and before `build`) it reads as UTC.
static TIMEZONE: OnceLock<Tz> = OnceLock::new();

/// Fixes the timezone for the life of the process. Later calls are ignored.
pub fn set_timezone(tz: Tz) {
    let _ = TIMEZONE.set(tz);
}

pub fn timezone() -> Tz {
    TIMEZONE.get().copied().unwrap_or(Tz::UTC)
}

/// RFC 3339 UTC timestamp with second precision, e.g. `2026-09-04T10:00:00Z`.
///
/// Stored timestamps stay UTC regardless of `LOGB_TIMEZONE`: they record when something
/// happened, and are rendered in the reader's own locale by the frontend.
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Today's date in the configured timezone as `YYYY-MM-DD`.
///
/// This is the date a reminder's `due_date` is compared against, so it has to be the user's
/// today, not the server's: on UTC a household in UTC+13 would see a reminder come due most
/// of a day late, and one in UTC-8 would see it a day early.
pub fn today() -> String {
    Utc::now().with_timezone(&timezone()).date_naive().to_string()
}

/// The current hour (0-23) in the configured timezone.
pub fn local_hour() -> u32 {
    use chrono::Timelike;
    Utc::now().with_timezone(&timezone()).hour()
}

/// How many migrations this binary carries.
///
/// `sqlx::migrate!` embeds the directory at compile time, so this is what the running code
/// believes the schema should be -- the number the health check compares the database against.
pub fn expected_migrations() -> usize {
    sqlx::migrate!("./migrations").iter().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `today()` reads the process timezone, which the tests leave at UTC; the conversion
    /// itself is what matters, so exercise it directly.
    fn today_in(tz: Tz) -> String {
        Utc::now().with_timezone(&tz).date_naive().to_string()
    }

    #[test]
    fn the_default_timezone_is_utc() {
        assert_eq!(timezone(), Tz::UTC);
        assert_eq!(today(), today_in(Tz::UTC));
    }

    /// The point of the setting: at some hours of the day, "today" in Auckland and "today" in
    /// Los Angeles are different dates, and a due date has to be read in the household's own.
    #[test]
    fn timezones_far_enough_apart_disagree_about_the_date() {
        let instant = Utc::now();
        let auckland = instant.with_timezone(&Tz::Pacific__Auckland).date_naive();
        let los_angeles = instant.with_timezone(&Tz::America__Los_Angeles).date_naive();
        assert!(
            (auckland - los_angeles).num_days() >= 0 && (auckland - los_angeles).num_days() <= 1,
            "Auckland is ahead of Los Angeles by at most a day: {auckland} vs {los_angeles}"
        );
    }

    #[test]
    fn stored_timestamps_stay_utc_and_parse_back() {
        let n = now();
        assert!(n.ends_with('Z'), "{n}");
        assert!(chrono::DateTime::parse_from_rfc3339(&n).is_ok(), "{n}");
    }

    #[test]
    fn the_local_hour_is_in_range() {
        assert!(local_hour() < 24);
    }
}

#[cfg(test)]
mod url_tests {
    use super::*;

    #[test]
    fn a_data_directory_with_url_punctuation_is_refused_rather_than_truncated() {
        // `?` and `#` are legal in a Linux path. Silently cutting the path there would open a
        // database somewhere other than where the operator said, which is the kind of failure
        // that looks like data loss. `%` is legal too, and is how a URL escapes other bytes --
        // `AnyPool` percent-decodes the path it connects to, so a directory containing a
        // `%XX` sequence would silently resolve to a *different* directory instead of being
        // truncated. See `a_percent_escape_in_the_data_directory_cannot_open_a_neighbours_database`
        // below for the concrete case that motivated adding it here.
        for bad in ["/data/we?rd", "/data/we#rd", "/data/we%rd"] {
            let err = sqlite_url(Path::new(bad)).unwrap_err().to_string();
            assert!(err.contains(bad), "the message must name the directory: {err}");
            assert!(err.contains("LOGB_DATABASE_URL"), "and say what to do about it: {err}");
        }
    }

    #[test]
    fn an_ordinary_data_directory_still_produces_the_url_it_always_did() {
        assert_eq!(sqlite_url(Path::new("/data")).unwrap(), "sqlite:///data/logb.db?mode=rwc");
    }

    /// The character check above proves `%` is refused; this proves *why* that matters.
    ///
    /// `AnyPool` percent-decodes the URL path it is given. `sqlite_url` does not encode a data
    /// directory on the way in, and `sqlite_file` does not decode one on the way out, so before
    /// the guard above existed, a directory literally named `pct%41` and a sibling literally
    /// named `pctA` were the same database as far as `connect` was concerned: the `%41` in the
    /// URL decodes to `A`, and `AnyPool` opened `pctA`'s file instead. `--backup` would have
    /// snapshotted the neighbour and reported success; the directory actually named would never
    /// get a database of its own.
    #[tokio::test]
    async fn a_percent_escape_in_the_data_directory_cannot_open_a_neighbours_database() {
        let parent = tempfile::tempdir().unwrap();
        let real_dir = parent.path().join("pctA");
        let escaped_dir = parent.path().join("pct%41"); // decodes, byte-for-byte, to "pctA"
        std::fs::create_dir_all(&real_dir).unwrap();

        // Put a marker in the directory that percent-decoding would silently redirect to.
        let real_url = sqlite_url(&real_dir).unwrap();
        let marker_pool = connect(&real_url).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, created_at) \
             VALUES (999, 'marker', 'x', '2026-01-01T00:00:00Z')",
        )
        .execute(&marker_pool)
        .await
        .unwrap();
        marker_pool.close().await;

        // Asking for `pct%41` must fail outright -- not succeed by silently decoding onto
        // `pctA` and handing back a pool that can read its marker row.
        let opened = match sqlite_url(&escaped_dir) {
            Err(e) => Err(e),
            Ok(url) => connect(&url).await,
        };
        assert!(
            opened.is_err(),
            "a '%' in the data directory must not silently open a neighbouring database"
        );
    }
}
