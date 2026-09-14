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
    /// The server's integer id for `entity_uuid`, so a device that first hears of a row here
    /// can relate it to the ids REST responses and file URLs use. Null once the row has been
    /// hard-purged by retention, by which point there is nothing to apply the change to.
    pub entity_id: Option<i64>,
}

pub async fn pull(
    db: &sqlx::AnyPool,
    user_id: i64,
    since: i64,
    limit: i64,
) -> Result<Vec<ChangeRow>, AppError> {
    Ok(sqlx::query_as::<_, ChangeRow>(
        "SELECT c.seq, c.entity, c.entity_uuid, c.op, c.field, c.value, c.edited_at, c.device_id, \
         CASE c.entity \
           WHEN 'object'     THEN (SELECT id FROM objects     WHERE client_uuid = c.entity_uuid) \
           WHEN 'activity'   THEN (SELECT id FROM activities  WHERE client_uuid = c.entity_uuid) \
           WHEN 'reminder'   THEN (SELECT id FROM reminders   WHERE client_uuid = c.entity_uuid) \
           WHEN 'attachment' THEN (SELECT id FROM attachments WHERE client_uuid = c.entity_uuid) \
           WHEN 'file'       THEN (SELECT id FROM files       WHERE client_uuid = c.entity_uuid) \
         END AS entity_id \
         FROM changes c WHERE c.user_id = $1 AND c.seq > $2 ORDER BY c.seq LIMIT $3")
        .bind(user_id)
        .bind(since)
        .bind(limit)
        .fetch_all(db)
        .await?)
}

/// The oldest `seq` still retained for this user, or 0 when this user's own log is empty.
///
/// A non-zero cursor sitting at 0 retained rows has nothing to resume from -- see the call
/// site in `api::sync::pull`, which is the only thing this specific case guards.
pub async fn horizon(db: &sqlx::AnyPool, user_id: i64) -> Result<i64, AppError> {
    let lowest: Option<i64> = sqlx::query_scalar("SELECT min(seq) FROM changes WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(db)
        .await?;
    Ok(lowest.unwrap_or(0))
}

/// The oldest `seq` retained anywhere in the log, across every user, or 0 when it is empty.
///
/// `seq` is one sequence shared by every account, handed out in commit order under
/// `db::begin_write`'s advisory lock, and `applied_at` is stamped in that same commit -- so the
/// two rise together and the purge's `DELETE FROM changes WHERE applied_at < $1` always removes
/// a prefix of `seq`, on every backend, never a hole further in. (A user's own account being
/// deleted can also remove rows, out of `seq` order, but only that user's -- it can raise this
/// floor, never lower what it means for anyone still here.) That makes this floor a sound proof
/// that nothing above it has been purged out from under ANY user, which is a stronger and
/// simpler question than "was this particular gap mine": a `client_op_id` a push rejected never
/// leaves a row for any statement to see (`api::sync::push` deletes the claim inside the same
/// transaction that inserted it), and another account's own ops were never this user's to lose
/// either way -- both just widen the numeric distance between two of this user's real rows
/// without a single one of them having gone missing. `api::sync::pull` is the only caller, and
/// `since >= this - 1` is the exact question "has the client's cursor fallen below what the
/// server still retains for anyone" -- not an approximation of it, which is what `horizon`
/// compared with a hard-coded "at most one missing" used to be.
pub async fn retention_floor(db: &sqlx::AnyPool) -> Result<i64, AppError> {
    let lowest: Option<i64> = sqlx::query_scalar("SELECT min(seq) FROM changes")
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
    //
    // The last guard on `objects` is the same promise turned inward: an object may name another
    // object as its `parent_id`, so the table is its own child table and the ordering of this
    // list cannot separate the two. It carries no `deleted_at` condition on `c` deliberately. A
    // LIVE child could not be there anyway -- deleting an object cascades tombstones over its
    // whole subtree, so no live child outlives its parent's tombstone -- but a child tombstoned
    // later than its parent, and so still inside the window, must hold the parent back all the
    // same: `objects.parent_id` references `objects(id)` with no `ON DELETE` action, so taking
    // the parent first, in any run where the child's own row survives that same statement --
    // its tombstone still fresh, or the child itself held back by one of the three guards above
    // -- fails the entire purge on the foreign key, and if that reference were ever relaxed it
    // would silently leave the child pointing at nothing.
    //
    // The guard is not reserved for the runs where parent and child part company: it applies to
    // the ordinary cascade too. An `ON DELETE CASCADE`-style tombstoning stamps a whole subtree
    // with one shared timestamp, so parent and child do age out together -- but the `NOT EXISTS`
    // subquery answers from the snapshot the statement began with, in which every descendant row
    // is still present, so the parent is held back all the same. A chain unwinds one level per
    // run, deepest first, always: measured on a four-deep chain with everything aged out, run 1
    // leaves 1, 2 and 3, run 2 leaves 1 and 2, run 3 leaves 1, run 4 leaves nothing. That is the
    // guard's whole cost, and it is what
    // `a_tombstoned_parent_is_not_purged_while_a_tombstoned_child_still_references_it` in
    // `tests/sync.rs` spells out as "one extra run per level of nesting". The parent waiting a
    // run is what it already does for an activity, a reminder or an attachment; nesting only
    // makes it happen more than once.
    //
    // What that same guard cannot unwind is a row that is its own descendant. There is no
    // `c.id <> objects.id` condition, so an object naming itself as its `parent_id` satisfies
    // its own `NOT EXISTS` on every run and stays in the table forever, as do both rows of a
    // two-object cycle -- with no error and no log line, which is why the `warn!` below exists
    // for the self-parenting case. Unreachable through any validated write path (both doors go
    // through `record::parent_is_valid`, and `objects::update` asks it under the write lock), so
    // only a hand-edited database or an import can produce one; and being held back forever is
    // strictly safer than the alternative the guard replaced, which was failing the whole purge
    // on a foreign key. It is left as it is rather than excluded, because a condition narrow
    // enough to release a self-parent would not release a two-object cycle, and one wide enough
    // for both is a recursive walk run per candidate row on every purge -- a real cost, every
    // run, for a state no write path can reach. The `warn!` is the trade: an operator can see it.
    let guards = [
        ("attachments", ""),
        ("activities", "AND NOT EXISTS (SELECT 1 FROM attachments c WHERE c.activity_id = activities.id)"),
        ("reminders", ""),
        ("objects", "AND NOT EXISTS (SELECT 1 FROM activities c WHERE c.object_id = objects.id) \
                     AND NOT EXISTS (SELECT 1 FROM reminders c WHERE c.object_id = objects.id) \
                     AND NOT EXISTS (SELECT 1 FROM attachments c WHERE c.object_id = objects.id) \
                     AND NOT EXISTS (SELECT 1 FROM objects c WHERE c.parent_id = objects.id)"),
    ];
    // One transaction around the whole purge, and every guarded read or delete against it. Run
    // as separate autocommit statements they were separate answers to "has this parent any
    // children left" or "what does this attachment still pin", each taken at its own moment: on
    // PostgreSQL a `NOT EXISTS` subquery answers from the snapshot its statement began with, so
    // a child committing while the statement waited on the parent's row lock was invisible to
    // the guard and still taken by the cascade -- hard-deleted with no tombstone and no
    // `changes` row, which is precisely the silent loss the guards are here to prevent. The same
    // gap between an autocommit read and a later autocommit delete could let an attachment cross
    // the cutoff after `pinned` was read but before its row (or its parent's) is removed, leaking
    // its blob until the next run. Inside `begin_write` the read, the guards and their deletes
    // all see one state, and no other writer can interleave: on PostgreSQL because the advisory
    // lock it takes is the one every write path takes, on SQLite because `BEGIN IMMEDIATE` holds
    // the only write lock the database has for the length of the transaction rather than for one
    // statement at a time.
    let mut tx = crate::db::begin_write(db, state.backend).await?;

    // Aged-out log rows, in the same transaction as everything below rather than run against
    // the pool first: nothing here reads `changes` afterwards, so a row landing in the gap
    // could not be lost the way an interleaved child could, but running it before the
    // transaction even opened would have made the comment above a lie about what "one
    // transaction around the whole purge" actually covered.
    let removed = sqlx::query("DELETE FROM changes WHERE applied_at < $1")
        .bind(&cutoff)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // Which files the expiring attachments were pinning, read BEFORE those rows go: once the
    // attachment is deleted the link is gone, and with it any way to find the blob to reclaim.
    let pinned: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT file_id FROM attachments \
         WHERE deleted_at IS NOT NULL AND deleted_at < $1")
        .bind(&cutoff)
        .fetch_all(&mut *tx)
        .await?;

    for (table, guard) in guards {
        // `table` and `guard` only ever come from the fixed list above, never from user input
        // -- exactly the audit `AssertSqlSafe` asks the author to have made before sqlx will
        // accept it.
        let sql =
            format!("DELETE FROM {table} WHERE deleted_at IS NOT NULL AND deleted_at < $1 {guard}");
        sqlx::query(sqlx::AssertSqlSafe(sql)).bind(&cutoff).execute(&mut *tx).await?;
    }

    // The one case of "held back forever" this run can name out loud. An aged-out object that is
    // its own parent satisfies the last `objects` guard against itself on every run: it is never
    // purged, and without this line nothing anywhere says so -- no error, no log, just a row that
    // quietly outlives its retention window. Read after the deletes, so what it reports is what
    // actually survived them rather than what was about to be attempted. One `SELECT` per purge
    // run, on an indexed column, against a table this app expects to hold a household's
    // belongings; a two-object cycle costs the same silence and is not detected here, for the
    // reason given on the guards above.
    let self_parents: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM objects WHERE deleted_at IS NOT NULL AND deleted_at < $1 AND parent_id = id")
        .bind(&cutoff)
        .fetch_all(&mut *tx)
        .await?;
    if !self_parents.is_empty() {
        tracing::warn!(
            objects = ?self_parents,
            "tombstoned objects name themselves as their own parent, so the purge's child guard \
             holds them back on every run and they will never be removed; no validated write \
             path can produce this, so the rows came from an import or a hand edit -- clearing \
             their parent_id lets the next purge take them",
        );
    }

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
    //
    // Run in the same transaction as the guarded deletes above, not against the pool afterwards:
    // the same guard-then-delete shape, straddled by a concurrent write, would otherwise lose a
    // field clock instead of a row -- the next edit to that field would then win on an empty
    // comparison instead of on its timestamp.
    sqlx::query(
        "DELETE FROM field_clock WHERE \
           NOT EXISTS (SELECT 1 FROM objects WHERE client_uuid = field_clock.entity_uuid) \
           AND NOT EXISTS (SELECT 1 FROM activities WHERE client_uuid = field_clock.entity_uuid) \
           AND NOT EXISTS (SELECT 1 FROM reminders WHERE client_uuid = field_clock.entity_uuid) \
           AND NOT EXISTS (SELECT 1 FROM attachments WHERE client_uuid = field_clock.entity_uuid) \
           AND NOT EXISTS (SELECT 1 FROM files WHERE client_uuid = field_clock.entity_uuid)")
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    // Now that no attachment references them, the unreferenced ones can give up their blob and
    // thumbnail. `purge_orphan_files` re-checks each candidate against the live attachments, so
    // a file still used by another object is left alone.
    //
    // After the commit, deliberately, and it cannot be moved inside the transaction above.
    // It works through the pool, so it would take a connection of its own -- one the
    // transaction is not holding, which a pool of one does not have -- and then ask that
    // connection to `DELETE FROM files` for rows whose `attachments` still exist as far as
    // every other connection is concerned, because this transaction's deletes have not
    // committed. `attachments.file_id` is `ON DELETE RESTRICT`, so that check blocks on the
    // uncommitted rows and waits for a transaction that is itself waiting for this call to
    // return: a hang rather than an error, and on SQLite the same standoff arrives as the
    // write lock the open transaction holds. It also removes blobs and thumbnails from disk,
    // which no rollback can put back, so the honest order is rows gone for good first, bytes
    // after.
    crate::api::attachments::purge_orphan_files(state, &pinned).await?;

    Ok(removed)
}
