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
    db: &sqlx::AnyPool,
    user_id: i64,
    since: i64,
    limit: i64,
) -> Result<Vec<ChangeRow>, AppError> {
    Ok(sqlx::query_as::<_, ChangeRow>(
        "SELECT seq, entity, entity_uuid, op, field, value, edited_at, device_id \
         FROM changes WHERE user_id = $1 AND seq > $2 ORDER BY seq LIMIT $3")
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
pub async fn horizon(db: &sqlx::AnyPool, user_id: i64) -> Result<i64, AppError> {
    let lowest: Option<i64> = sqlx::query_scalar("SELECT min(seq) FROM changes WHERE user_id = $1")
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
    db: &sqlx::AnyPool,
    user_id: i64,
) -> Result<(i64, serde_json::Value), AppError> {
    let seq: i64 = sqlx::query_scalar("SELECT coalesce(max(seq), 0) FROM changes WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(db)
        .await?;

    let objects = rows(db, "SELECT * FROM objects WHERE user_id = $1 AND deleted_at IS NULL", user_id).await?;
    let activities = rows(db,
        "SELECT a.* FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL", user_id).await?;
    let reminders = rows(db,
        "SELECT r.* FROM reminders r JOIN objects o ON o.id = r.object_id \
         WHERE o.user_id = $1 AND r.deleted_at IS NULL AND o.deleted_at IS NULL", user_id).await?;
    let attachments = rows(db,
        "SELECT t.* FROM attachments t JOIN objects o ON o.id = t.object_id \
         WHERE o.user_id = $1 AND t.deleted_at IS NULL AND o.deleted_at IS NULL", user_id).await?;
    let files = rows(db, "SELECT * FROM files WHERE user_id = $1 AND deleted_at IS NULL", user_id).await?;

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
    db: &sqlx::AnyPool,
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
                // `Any` reports its own type names, not the driver's: what the SQLite
                // driver called INTEGER and REAL arrive here as BIGINT and DOUBLE. Both
                // spellings are listed so this reads the same rows it always did, and the
                // names PostgreSQL will produce are listed alongside them.
                match raw.type_info().name() {
                    "BIGINT" | "INTEGER" | "SMALLINT" => serde_json::json!(row.try_get::<i64, _>(i)?),
                    "DOUBLE" | "REAL" => serde_json::json!(row.try_get::<f64, _>(i)?),
                    "BOOLEAN" => serde_json::json!(row.try_get::<bool, _>(i)?),
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
///
/// That promise is only kept if no row can be removed through a parent's `ON DELETE CASCADE`
/// ahead of its own expiry, so a parent whose children have not all aged out is skipped and
/// tried again next run. See the guards on the delete loop below.
pub async fn purge(
    state: &crate::state::App,
    retention_days: i64,
) -> Result<u64, AppError> {
    let db = &state.db;
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(retention_days))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let removed = sqlx::query("DELETE FROM changes WHERE applied_at < $1")
        .bind(&cutoff)
        .execute(db)
        .await?
        .rows_affected();

    // Which files the expiring attachments were pinning, read BEFORE those rows go: once the
    // attachment is deleted the link is gone, and with it any way to find the blob to reclaim.
    let pinned: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT file_id FROM attachments \
         WHERE deleted_at IS NOT NULL AND deleted_at < $1")
        .bind(&cutoff)
        .fetch_all(db)
        .await?;

    // `files` is absent deliberately: `files.deleted_at` is never set, because
    // `sync::apply::apply_op`'s Delete arm refuses a `delete` op on `Entity::File` outright, so
    // no code path ever produces a file tombstone for this loop to find. A file is
    // content-addressed and shared between attachments; it dies when its last attachment does,
    // which is what `purge_orphan_files` decides below. That refusal is the only reason this
    // guards array may skip `files` -- if it were ever loosened, a `files` entry would have to
    // be added here too, or a tombstoned file would sit unpurged forever.
    //
    // The `NOT EXISTS` guards are defence in depth, and the reason the order below matters.
    // `foreign_keys(true)` plus the schema's `ON DELETE CASCADE` means the hard delete of a
    // parent takes every child with it, whatever state that child is in. A delete path that
    // forgets to cascade tombstones -- as `sync::apply::apply_op` once did -- therefore turns
    // this purge into a silent destroyer of LIVE rows: gone with no tombstone and no `changes`
    // entry, so no device ever learns they existed, and a destroyed attachment strands its
    // `files` row where the pinned list above (built from tombstoned attachments only) can
    // never see it, leaking the blob on disk forever.
    //
    // Children are purged before their parents, so by the time a parent is considered every
    // child tombstone that has aged out is already gone. Any child row still present is
    // therefore either live or a tombstone still inside the window -- exactly the two kinds
    // this function promises not to destroy early. So the parent waits instead. Waiting is
    // recoverable: the row is still there next run, and the anomaly stays visible. Tombstoning
    // the stragglers here instead would stamp them with today's date and could not log them --
    // the log rows for their window are already gone, and a purge has no device to attribute
    // them to -- so no device would ever learn of them and the next run would destroy them for
    // good. That is the same silent loss wearing a tidier hat.
    let guards = [
        ("attachments", ""),
        ("activities", "AND NOT EXISTS (SELECT 1 FROM attachments c WHERE c.activity_id = activities.id)"),
        ("reminders", ""),
        ("objects", "AND NOT EXISTS (SELECT 1 FROM activities c WHERE c.object_id = objects.id) \
                     AND NOT EXISTS (SELECT 1 FROM reminders c WHERE c.object_id = objects.id) \
                     AND NOT EXISTS (SELECT 1 FROM attachments c WHERE c.object_id = objects.id)"),
    ];
    for (table, guard) in guards {
        // `table` and `guard` only ever come from the fixed list above, never from user input
        // -- exactly the audit `AssertSqlSafe` asks the author to have made before sqlx will
        // accept it.
        let sql =
            format!("DELETE FROM {table} WHERE deleted_at IS NOT NULL AND deleted_at < $1 {guard}");
        sqlx::query(sqlx::AssertSqlSafe(sql)).bind(&cutoff).execute(db).await?;
    }

    // Now that no attachment references them, the unreferenced ones can give up their blob and
    // thumbnail. `purge_orphan_files` re-checks each candidate against the live attachments, so
    // a file still used by another object is left alone.
    crate::api::attachments::purge_orphan_files(state, &pinned).await?;

    // A field clock for a row nobody can name any more is dead weight.
    //
    // A correlated `NOT EXISTS` per table, not `entity_uuid NOT IN (SELECT client_uuid ...)`.
    // `client_uuid` is nullable on all five tables, and `x NOT IN (subquery)` evaluates to
    // UNKNOWN -- never true -- for EVERY row the moment that subquery yields a single NULL. So
    // one row anywhere in the database without a uuid disabled this sweep entirely, for every
    // genuinely orphaned clock, permanently and with nothing to show for it. `NOT EXISTS` asks
    // the only question that matters, "does any row still carry this uuid", and a NULL uuid
    // simply never matches -- which is right, since a row with no uuid is not one a
    // `field_clock` row could have been naming.
    sqlx::query(
        "DELETE FROM field_clock WHERE \
           NOT EXISTS (SELECT 1 FROM objects WHERE client_uuid = field_clock.entity_uuid) \
           AND NOT EXISTS (SELECT 1 FROM activities WHERE client_uuid = field_clock.entity_uuid) \
           AND NOT EXISTS (SELECT 1 FROM reminders WHERE client_uuid = field_clock.entity_uuid) \
           AND NOT EXISTS (SELECT 1 FROM attachments WHERE client_uuid = field_clock.entity_uuid) \
           AND NOT EXISTS (SELECT 1 FROM files WHERE client_uuid = field_clock.entity_uuid)")
        .execute(db)
        .await?;

    Ok(removed)
}
