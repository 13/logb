//! The identity of the database a cursor refers to. See `migrations/0008_sync_epoch.sql`.

use crate::error::AppError;

pub async fn current(db: &sqlx::SqlitePool) -> Result<String, AppError> {
    let (v,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'sync_epoch'")
        .fetch_one(db)
        .await?;
    Ok(v)
}

/// Gives the database a new identity, invalidating every cursor held anywhere.
///
/// Called by `--restore`. It is deliberately not reachable over HTTP: rotating without
/// replacing the data would send every device to a bootstrap for no reason.
pub async fn rotate(db: &sqlx::SqlitePool) -> Result<String, AppError> {
    let fresh = uuid::Uuid::new_v4().to_string();
    sqlx::query("UPDATE settings SET value = ? WHERE key = 'sync_epoch'")
        .bind(&fresh)
        .execute(db)
        .await?;
    Ok(fresh)
}
