//! The identity of the database a cursor refers to. See `migrations/0008_sync_epoch.sql`.

use crate::error::AppError;

pub async fn current(db: &sqlx::AnyPool) -> Result<String, AppError> {
    let (v,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'sync_epoch'")
        .fetch_one(db)
        .await?;
    Ok(v)
}

/// Gives the database a new identity, invalidating every cursor held anywhere.
///
/// Called by `--restore`. It is deliberately not reachable over HTTP: rotating without
/// replacing the data would send every device to a bootstrap for no reason.
///
/// An upsert, not a plain `UPDATE`: a bare `UPDATE ... WHERE key = 'sync_epoch'` matches zero
/// rows if that row is ever absent (a restored snapshot old enough to predate it, or the row
/// having been lost some other way), and would still report the freshly minted uuid as the new
/// epoch while the database went on advertising its old identity -- or none at all, which then
/// makes every pull 500 through `current`'s `fetch_one`. The `rows_affected` assertion is the
/// invariant this relies on: for a single primary-keyed row, insert-or-update always affects
/// exactly one row, so anything else means the write did not do what it claims.
pub async fn rotate(db: &sqlx::AnyPool) -> Result<String, AppError> {
    let fresh = uuid::Uuid::new_v4().to_string();
    let result = sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('sync_epoch', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(&fresh)
        .execute(db)
        .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Internal(format!(
            "sync epoch rotation affected {} rows, not 1 -- the epoch was not reliably set",
            result.rows_affected()
        )));
    }
    Ok(fresh)
}
