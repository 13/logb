//! Reading the change log: the pull cursor and the retention horizon.

use crate::error::AppError;
use serde::Serialize;

#[derive(Serialize, sqlx::FromRow, Debug, Clone)]
pub struct ChangeRow {
    pub seq: i64,
    pub entity: String,
    pub entity_uuid: String,
    pub op: String,
    pub field: Option<String>,
    pub value: Option<String>,
    pub edited_at: String,
    pub device_id: String,
}

pub async fn pull(
    db: &sqlx::SqlitePool,
    user_id: i64,
    since: i64,
    limit: i64,
) -> Result<Vec<ChangeRow>, AppError> {
    Ok(sqlx::query_as::<_, ChangeRow>(
        "SELECT seq, entity, entity_uuid, op, field, value, edited_at, device_id \
         FROM changes WHERE user_id = ? AND seq > ? ORDER BY seq LIMIT ?")
        .bind(user_id)
        .bind(since)
        .bind(limit)
        .fetch_all(db)
        .await?)
}

/// The oldest `seq` still retained for this user, or 0 when the log is empty.
///
/// A client whose cursor sits below this has missed ops that were purged, so an incremental
/// pull would silently skip them -- it has to re-bootstrap instead.
pub async fn horizon(db: &sqlx::SqlitePool, user_id: i64) -> Result<i64, AppError> {
    let lowest: Option<i64> = sqlx::query_scalar("SELECT min(seq) FROM changes WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(db)
        .await?;
    Ok(lowest.unwrap_or(0))
}
