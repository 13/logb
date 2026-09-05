use chrono::{SecondsFormat, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::time::Duration;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub async fn connect(data_dir: &Path) -> Result<SqlitePool, BoxError> {
    std::fs::create_dir_all(data_dir)?;
    let opts = SqliteConnectOptions::new()
        .filename(data_dir.join("memto.db"))
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

/// RFC 3339 UTC timestamp with second precision, e.g. `2026-09-04T10:00:00Z`.
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Today's date in UTC as `YYYY-MM-DD`.
pub fn today() -> String {
    Utc::now().date_naive().to_string()
}
