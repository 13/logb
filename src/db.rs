use chrono::{SecondsFormat, Utc};
use chrono_tz::Tz;
use std::sync::OnceLock;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::time::Duration;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub async fn connect(data_dir: &Path) -> Result<SqlitePool, BoxError> {
    std::fs::create_dir_all(data_dir)?;
    let opts = SqliteConnectOptions::new()
        .filename(data_dir.join("logby.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Opens an existing database without running migrations, for read-only side commands such
/// as `--backup` that must not touch the schema of a running instance.
pub async fn connect_existing(data_dir: &Path) -> Result<SqlitePool, BoxError> {
    let path = data_dir.join("logby.db");
    if !path.exists() {
        return Err(format!("no database at {}", path.display()).into());
    }
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(30));
    Ok(SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?)
}

/// Writes a consistent snapshot of the database to `dest`.
///
/// `VACUUM INTO` is the reason this exists: copying `logby.db` out from under a running
/// instance can catch it mid-write and miss the WAL entirely, while this runs inside a read
/// transaction and produces a compacted, self-consistent file.
pub async fn backup_to(pool: &SqlitePool, dest: &Path) -> Result<(), BoxError> {
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()).into());
    }
    let dest = dest.to_str().ok_or("backup path must be valid UTF-8")?;
    sqlx::query("VACUUM INTO ?").bind(dest).execute(pool).await?;
    Ok(())
}

/// The instance's wall-clock timezone, set once from `LOGBY_TIMEZONE` at startup.
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
/// Stored timestamps stay UTC regardless of `LOGBY_TIMEZONE`: they record when something
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
