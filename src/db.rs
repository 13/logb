use chrono::{NaiveDate, SecondsFormat, Utc};
use chrono_tz::Tz;
use sqlx::any::AnyPoolOptions;
use sqlx::AnyPool;
use sqlx::Executor;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

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
/// Used to keep the two things a URL cannot say: that a data directory has to exist before
/// SQLite can create a file in it, and that `connect_existing` must find a database rather
/// than make one. `pub(crate)` so `restore::run` can locate the live database file from the
/// URL it is handed, instead of re-deriving a URL from a data directory of its own.
pub(crate) fn sqlite_file(url: &str) -> Option<PathBuf> {
    let rest = url
        .strip_prefix("sqlite://")
        .or_else(|| url.strip_prefix("sqlite:"))?;
    let path = rest.split(['?', '#']).next().unwrap_or("");
    (!path.is_empty() && path != ":memory:").then(|| PathBuf::from(path))
}

/// A connection URL with everything secret taken out of it, for an error message or a log line.
///
/// `postgres://user:pass@host:5432/logb?sslmode=require` becomes `postgres://…@host:5432/logb`.
/// The whole userinfo goes, not just the password -- a username is a hint about the password --
/// and so does the query string, which is another place a credential can hide (`?password=`).
/// A URL LogB cannot recognise is reduced to its scheme rather than guessed at: printing an
/// unparsed string is exactly how a password ends up in a log.
pub fn redacted(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return match url.split_once(':') {
            Some((scheme, _)) => format!("{scheme}:…"),
            None => "…".to_string(),
        };
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let (credentials, host) = match authority.rsplit_once('@') {
        Some((_, host)) => ("…@", host),
        None => ("", authority),
    };
    let path = tail.split(['?', '#']).next().unwrap_or("");
    format!("{scheme}://{credentials}{host}{path}")
}

/// The same scrubbing, applied to text that came from somewhere else -- a driver error, which
/// may quote the URL it was given back at whoever is reading the log.
///
/// sqlx 0.9 does not, today: a refused connection is `error communicating with database:
/// Connection refused`. That is not a guarantee, and it is not true of every error a driver can
/// raise, so any text that is about to be joined to a URL is put through here first.
pub(crate) fn scrub(text: &str, url: &str) -> String {
    let mut out = text.replace(url, &redacted(url));
    if let Some((_, rest)) = url.split_once("://") {
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        if let Some((credentials, _)) = rest[..authority_end].rsplit_once('@') {
            if !credentials.is_empty() {
                out = out.replace(credentials, "…");
                if let Some((_, password)) = credentials.split_once(':') {
                    if !password.is_empty() {
                        out = out.replace(password, "…");
                    }
                }
            }
        }
    }
    out
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
    AnyPoolOptions::new()
        .max_connections(max_connections)
        .after_connect(move |conn, _meta| {
            let pragmas = pragmas.clone();
            Box::pin(async move {
                if let Some(sql) = pragmas {
                    conn.execute(sqlx::AssertSqlSafe(sql)).await?;
                }
                Ok(())
            })
        })
}

/// Connects using the default pool size for the backend: 4 for SQLite, 16 otherwise. Most
/// callers have no reason to pick a different size; `connect_with_pool_size` is for the one
/// that does (the test harness, which needs a small pool to avoid exhausting a shared
/// PostgreSQL server run in parallel by many tests).
pub async fn connect(url: &str) -> Result<AnyPool, BoxError> {
    connect_with_pool_size(url, None).await
}

/// As `connect`, but `pool_size` overrides the backend's default max connections when set.
pub async fn connect_with_pool_size(
    url: &str,
    pool_size: Option<u32>,
) -> Result<AnyPool, BoxError> {
    sqlx::any::install_default_drivers();
    // SQLite will create the database file, but not the directory holding it.
    if let Some(file) = sqlite_file(url) {
        if let Some(dir) = file.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
    }
    let default_size = if url.starts_with("sqlite:") { 4 } else { 16 };
    let pool = pool_options(url, pool_size.unwrap_or(default_size), 5_000)
        .connect(url)
        .await?;
    migrator(url).run(&pool).await?;
    seed_settings(&pool).await?;
    Ok(pool)
}

/// Begins a transaction that intends to write, and makes it the only one.
///
/// Every write path goes through here. See `dialect::Backend::write_lock` for why PostgreSQL
/// needs more than a `BEGIN`.
pub async fn begin_write(
    pool: &AnyPool,
    backend: crate::dialect::Backend,
) -> Result<sqlx::Transaction<'static, sqlx::Any>, sqlx::Error> {
    let mut tx = pool.begin_with(backend.begin_write()).await?;
    if let Some(lock) = backend.write_lock() {
        sqlx::query(lock).execute(&mut *tx).await?;
    }
    Ok(tx)
}

/// The two `settings` rows the app cannot run without, written on first start if absent.
///
/// They used to be seeded by the SQLite migrations -- `currency` by 0001, `sync_epoch` by 0008
/// with `INSERT ... lower(hex(randomblob(16)))`. Neither spelling ports: PostgreSQL has no
/// `randomblob`, and a schema that seeds its own data would have to be kept in step with a
/// second copy of these defaults. The PostgreSQL schema therefore seeds nothing and this is the
/// one code path that produces both rows on either backend.
///
/// Without the `sync_epoch` row a fresh PostgreSQL database answers every sync pull and push
/// with a 500, because `epoch::current` is a `fetch_one`; without `currency` the settings
/// endpoint does the same.
///
/// `ON CONFLICT DO NOTHING` -- both dialects understand it -- is what makes this safe to run on
/// every start, and, more importantly, what stops it overwriting an epoch that already exists.
/// Minting a new one on each boot would silently send every device on a full re-bootstrap.
async fn seed_settings(pool: &AnyPool) -> Result<(), BoxError> {
    for (key, value) in [
        ("sync_epoch", crate::sync::epoch::fresh()),
        ("currency", "EUR".to_string()),
    ] {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ($1, $2) ON CONFLICT (key) DO NOTHING",
        )
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    }
    Ok(())
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
        }
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
    sqlx::query("VACUUM INTO $1")
        .bind(dest)
        .execute(pool)
        .await?;
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

/// The instance's wall-clock timezone.
///
/// A process-wide value rather than a parameter because `today()` is called from places with
/// no access to the config -- notably the `ReminderRow -> ReminderOut` conversion that decides
/// whether a reminder is due. Unset (in tests, and before `build`) it reads as UTC.
///
/// A lock rather than a `OnceLock`, because it is no longer only a startup setting: with no
/// `LOGB_TIMEZONE`, first-run setup stores the browser's timezone and an administrator can change
/// it in Settings, and either takes effect without a restart.
static TIMEZONE: RwLock<Option<Tz>> = RwLock::new(None);

/// Key in the `settings` table holding the timezone setup or an administrator chose.
pub const TIMEZONE_KEY: &str = "timezone";

pub fn set_timezone(tz: Tz) {
    // A poisoned lock only means a writer panicked mid-assignment of a `Copy` value, which
    // cannot leave it half-written; the value inside is still a whole `Option<Tz>`.
    *TIMEZONE.write().unwrap_or_else(|e| e.into_inner()) = Some(tz);
}

pub fn timezone() -> Tz {
    TIMEZONE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .unwrap_or(Tz::UTC)
}

/// Decides the timezone at startup: `LOGB_TIMEZONE` when it is set, otherwise the one stored in
/// `settings`, otherwise UTC. An environment variable is a deliberate operator choice and wins
/// over anything clicked in the app; Settings says so rather than offering a field that would do
/// nothing.
pub async fn load_timezone(configured: Option<Tz>, pool: &AnyPool) -> Result<(), BoxError> {
    if let Some(tz) = configured {
        set_timezone(tz);
        return Ok(());
    }
    let stored: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = $1")
        .bind(TIMEZONE_KEY)
        .fetch_optional(pool)
        .await?;
    match stored.as_deref().map(str::parse::<Tz>) {
        Some(Ok(tz)) => set_timezone(tz),
        Some(Err(_)) => {
            tracing::warn!(value = ?stored, "the stored timezone is not an IANA name; using UTC")
        }
        None => {}
    }
    Ok(())
}

/// RFC 3339 UTC timestamp with second precision, e.g. `2026-09-04T10:00:00Z`.
///
/// Stored timestamps stay UTC regardless of `LOGB_TIMEZONE`: they record when something
/// happened, and are rendered in the reader's own locale by the frontend.
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The zone a user's dates are read in: their own when they chose a valid one under
/// Settings → Notifications (`users.notify_tz`), the instance's otherwise. A stored name that
/// no longer parses is treated as unset rather than failing the request.
pub fn zone(tz: Option<&str>) -> Tz {
    tz.and_then(|raw| raw.parse::<Tz>().ok()).unwrap_or_else(timezone)
}

/// Today in `zone(tz)`. This is the date a reminder's `due_date` is compared against for that
/// user; `today()` below is the instance-wide answer for everything that is not per user.
pub fn today_in(tz: Option<&str>) -> NaiveDate {
    Utc::now().with_timezone(&zone(tz)).date_naive()
}

/// Today's date in the configured timezone as `YYYY-MM-DD`.
///
/// This is the instance's today: file names, delivery-history pruning and every stats bucket use
/// it. A reminder's due date is read against `today_in` with the user's own zone instead.
pub fn today() -> String {
    today_in(None).to_string()
}

/// The current hour (0-23) in the configured timezone.
pub fn local_hour() -> u32 {
    use chrono::Timelike;
    Utc::now().with_timezone(&timezone()).hour()
}

/// Which set of migrations this URL needs.
///
/// `migrate!` embeds the files at compile time, so both directories ship in the binary and the
/// only choice made here is which embedded set to run. The two sets are not translations of
/// each other: SQLite keeps its nine historical steps because existing databases have to be
/// moved forward one at a time, while a fresh PostgreSQL database has no history to replay and
/// gets today's schema in a single file. `tests/schema_parity.rs` is what keeps them agreeing.
pub fn migrator(url: &str) -> sqlx::migrate::Migrator {
    if url.starts_with("sqlite:") {
        sqlx::migrate!("./migrations/sqlite")
    } else {
        sqlx::migrate!("./migrations/postgres")
    }
}

/// How many migrations this binary carries for `url`'s backend.
///
/// Backend-dependent because the two sets have different lengths on purpose -- nine steps of
/// SQLite history against one PostgreSQL schema file -- so the health check must compare a
/// database against its own set, not the other one's.
pub fn expected_migrations(url: &str) -> usize {
    migrator(url).iter().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shape a connection URL takes here, checked for the one thing that matters: the
    /// password is gone, and enough of the URL survives to tell an operator which database
    /// this was about.
    #[test]
    fn a_redacted_url_keeps_the_database_and_loses_the_credentials() {
        for (url, expected) in [
            (
                "postgres://user:pass@host:5432/logb",
                "postgres://…@host:5432/logb",
            ),
            (
                "postgres://user:pass@host/logb?sslmode=require",
                "postgres://…@host/logb",
            ),
            // A password is not required for there to be a userinfo to hide.
            ("postgres://user@host/logb", "postgres://…@host/logb"),
            // No credentials at all: nothing to take out, and the host still says which.
            ("postgres://host/logb", "postgres://host/logb"),
            (
                "sqlite:///var/lib/logb/logb.db?mode=rwc",
                "sqlite:///var/lib/logb/logb.db",
            ),
            // Not a URL LogB can take apart -- say only the scheme rather than guess.
            ("sqlite:logb.db", "sqlite:…"),
            ("nonsense", "…"),
        ] {
            assert_eq!(redacted(url), expected, "redacting {url}");
        }
    }

    /// Driver text is scrubbed against the URL it was produced from, whether it quotes the whole
    /// URL or only the credentials in it. sqlx 0.9 does neither today; that is not a promise.
    #[test]
    fn scrubbing_removes_the_password_from_text_that_quotes_the_url() {
        let url = "postgres://user:hunter2@host:5432/logb";
        for quoted in [
            format!("failed to connect to {url}: refused"),
            "authentication failed for user:hunter2@host".to_string(),
            "the password hunter2 was rejected".to_string(),
        ] {
            let scrubbed = scrub(&quoted, url);
            assert!(
                !scrubbed.contains("hunter2"),
                "the password survived scrubbing: {scrubbed}"
            );
        }
    }

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
        let los_angeles = instant
            .with_timezone(&Tz::America__Los_Angeles)
            .date_naive();
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

    #[test]
    fn a_users_zone_falls_back_to_the_instance_zone() {
        use chrono_tz::Tz;
        assert_eq!(zone(Some("Europe/Berlin")), Tz::Europe__Berlin);
        assert_eq!(zone(Some("Mars/Olympus")), timezone());
        assert_eq!(zone(None), timezone());
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
            assert!(
                err.contains(bad),
                "the message must name the directory: {err}"
            );
            assert!(
                err.contains("LOGB_DATABASE_URL"),
                "and say what to do about it: {err}"
            );
        }
    }

    #[test]
    fn an_ordinary_data_directory_still_produces_the_url_it_always_did() {
        assert_eq!(
            sqlite_url(Path::new("/data")).unwrap(),
            "sqlite:///data/logb.db?mode=rwc"
        );
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
