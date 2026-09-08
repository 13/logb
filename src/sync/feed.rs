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

/// Every live row the user owns, plus the cursor to resume incremental pulls from.
///
/// The cursor is read BEFORE the rows. Taken afterwards, an op landing between the two reads
/// would be baked into the snapshot and then delivered again by the first pull -- harmless for
/// an idempotent apply, but it would also let an op that landed between them be skipped if the
/// order were reversed. Reading it first can only ever repeat work, never lose it.
pub async fn snapshot(
    db: &sqlx::SqlitePool,
    user_id: i64,
) -> Result<(i64, serde_json::Value), AppError> {
    let seq: i64 = sqlx::query_scalar("SELECT coalesce(max(seq), 0) FROM changes WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(db)
        .await?;

    let objects = rows(db, "SELECT * FROM objects WHERE user_id = ? AND deleted_at IS NULL", user_id).await?;
    let activities = rows(db,
        "SELECT a.* FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = ? AND a.deleted_at IS NULL AND o.deleted_at IS NULL", user_id).await?;
    let reminders = rows(db,
        "SELECT r.* FROM reminders r JOIN objects o ON o.id = r.object_id \
         WHERE o.user_id = ? AND r.deleted_at IS NULL AND o.deleted_at IS NULL", user_id).await?;
    let attachments = rows(db,
        "SELECT t.* FROM attachments t JOIN objects o ON o.id = t.object_id \
         WHERE o.user_id = ? AND t.deleted_at IS NULL AND o.deleted_at IS NULL", user_id).await?;
    let files = rows(db, "SELECT * FROM files WHERE user_id = ? AND deleted_at IS NULL", user_id).await?;

    Ok((seq, serde_json::json!({
        "objects": objects,
        "activities": activities,
        "reminders": reminders,
        "attachments": attachments,
        "files": files,
    })))
}

/// Reads a whole table as JSON objects, column names taken from the result set.
///
/// Generic in the shape it returns because the snapshot ships rows verbatim -- a typed struct
/// per table would have to be kept in step with five schemas for no gain to any caller.
async fn rows(
    db: &sqlx::SqlitePool,
    sql: &'static str,
    user_id: i64,
) -> Result<Vec<serde_json::Value>, AppError> {
    use sqlx::{Column, Row, TypeInfo, ValueRef};
    let fetched = sqlx::query(sql).bind(user_id).fetch_all(db).await?;
    let mut out = Vec::with_capacity(fetched.len());
    for row in fetched {
        let mut map = serde_json::Map::new();
        for (i, col) in row.columns().iter().enumerate() {
            let raw = row.try_get_raw(i)?;
            let value = if raw.is_null() {
                serde_json::Value::Null
            } else {
                match raw.type_info().name() {
                    "INTEGER" => serde_json::json!(row.try_get::<i64, _>(i)?),
                    "REAL" => serde_json::json!(row.try_get::<f64, _>(i)?),
                    _ => serde_json::json!(row.try_get::<String, _>(i)?),
                }
            };
            map.insert(col.name().to_string(), value);
        }
        out.push(serde_json::Value::Object(map));
    }
    Ok(out)
}

/// Drops log rows and tombstones older than the retention window.
///
/// Tombstones go last and only once their own `deleted_at` has aged out, so a device that is
/// still inside the window always finds the delete in the log before the row itself vanishes.
/// Beyond the window a device has to re-bootstrap anyway, which is precisely when it no longer
/// needs the tombstone to learn the row is gone.
pub async fn purge(
    state: &crate::state::App,
    retention_days: i64,
) -> Result<u64, AppError> {
    let db = &state.db;
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(retention_days))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let removed = sqlx::query("DELETE FROM changes WHERE applied_at < ?")
        .bind(&cutoff)
        .execute(db)
        .await?
        .rows_affected();

    // Which files the expiring attachments were pinning, read BEFORE those rows go: once the
    // attachment is deleted the link is gone, and with it any way to find the blob to reclaim.
    let pinned: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT file_id FROM attachments \
         WHERE deleted_at IS NOT NULL AND deleted_at < ?")
        .bind(&cutoff)
        .fetch_all(db)
        .await?;

    // `files` is absent deliberately: nothing sets `files.deleted_at`, because a file is
    // content-addressed and shared between attachments. A file dies when its last attachment
    // does, which is what `purge_orphan_files` decides below.
    for table in ["attachments", "activities", "reminders", "objects"] {
        // `table` only ever comes from the fixed list above, never from user input -- exactly
        // the audit `AssertSqlSafe` asks the author to have made before sqlx will accept it.
        let sql = format!("DELETE FROM {table} WHERE deleted_at IS NOT NULL AND deleted_at < ?");
        sqlx::query(sqlx::AssertSqlSafe(sql)).bind(&cutoff).execute(db).await?;
    }

    // Now that no attachment references them, the unreferenced ones can give up their blob and
    // thumbnail. `purge_orphan_files` re-checks each candidate against the live attachments, so
    // a file still used by another object is left alone.
    crate::api::attachments::purge_orphan_files(state, &pinned).await?;

    // A field clock for a row nobody can name any more is dead weight.
    sqlx::query(
        "DELETE FROM field_clock WHERE entity_uuid NOT IN (\
           SELECT client_uuid FROM objects UNION ALL SELECT client_uuid FROM activities \
           UNION ALL SELECT client_uuid FROM reminders UNION ALL SELECT client_uuid FROM attachments \
           UNION ALL SELECT client_uuid FROM files)")
        .execute(db)
        .await?;

    Ok(removed)
}
