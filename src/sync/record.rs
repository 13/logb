//! Makes REST writes visible to the sync log.
//!
//! `sync::apply::apply_op` is only reached by `POST /sync/push`. Every other way a row
//! changes -- `POST`/`PATCH`/`DELETE` under `api::objects`, `api::activities`,
//! `api::reminders`, `api::attachments` and the bulk import in `api::export` -- used to write
//! straight to its table and never touch `changes` or `field_clock`. That produced two
//! failures: a device had nothing to pull after a browser edit or delete (`GET /sync/pull`
//! stayed silent, and a deleted object never expired off a phone), and a REST write left
//! `field_clock` holding a stale timestamp that a since-superseded sync op could still beat,
//! silently overwriting a newer edit. This module is what a REST handler calls, inside the
//! same transaction as the write it describes, to close both gaps.
//!
//! The delete cascade (`cascade_object`, `cascade_activity`, `clear_cover_of`, `log_cascade`)
//! also lives here rather than in `sync::apply`, and is called by *both* `apply_op` and the
//! REST delete handlers. It used to be hand-rolled a second time in each REST handler, and the
//! two copies drifted twice on this branch (a missed `done_activity_id` unlink, a missed cover
//! clear), each time caught only in review. One copy, called from both places, is what keeps
//! that from happening a third time.

use crate::error::AppError;
use crate::sync::{Entity, OpKind};

/// Identifies the REST path as an LWW participant in `field_clock` and `changes`.
///
/// A synced op carries a `device_id` the pushing device chose; REST has no such identity to
/// offer, so every REST write shares this one constant instead. Its only behavioural role is
/// `wins`'s device-id tie-break for two edits landing at the exact same millisecond (see
/// `sync::apply::wins`) -- nothing else reads it, and it is never compared against a real
/// device's id for any other purpose.
pub(crate) const DEVICE_ID: &str = "rest";

/// The current instant, in the one canonical form `wins` can compare (see rule 1 in the module
/// docs of `sync::apply`: `wins` compares `edited_at` lexically, which is only chronological
/// when every stored value shares the same width and zone spelling -- UTC, fixed millisecond
/// precision, trailing `Z`, exactly what `canonical_edited_at` produces for a client's own
/// `edited_at`). A REST write's clock has to land in that identical shape, not merely a
/// similar-looking one, or `wins`'s comparison against it is meaningless.
///
/// Deliberately NOT `canonical_edited_at(&db::now())`, even though that also yields the right
/// shape: `db::now()` is `SecondsFormat::Secs`, so every millisecond digit it could have kept
/// is already gone before `canonical_edited_at` ever sees it, and what comes out the other end
/// is always `…:SS.000Z` -- the right FORM wrapped around a value that is never later than the
/// top of its own second, up to 999ms earlier than the write actually happened. A phone op
/// edited at `…:SS.400Z`, genuinely earlier than a browser edit at `…:SS.900Z`, still beat it
/// under `wins` when the browser's own clock only ever remembered `…:SS.000Z` -- the exact lost
/// update this module exists to close, surviving inside the one-second window. Reading the
/// clock directly at the millisecond precision `wins` actually compares at is the fix: it is
/// the point, not an optimisation on top of the shape being right.
pub(crate) fn edited_at_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The `client_uuid` of `entity`'s row `id`.
///
/// Every row has carried one since migration `0007_sync.sql` backfilled every existing row and
/// every insert path began minting a fresh v4 for every new one -- there is no code path left
/// that creates a row without it. That makes it safe to treat as an invariant here rather than
/// as an `Option` a caller has to plan around, the same way `insights::read` treats a
/// server-generated date as always parseable. (Contrast `apply::nameable`, which *does* still
/// treat a cascaded CHILD's uuid as possibly absent -- that row's existence is not this
/// function's to guarantee, only its own.)
pub(crate) async fn uuid_of(
    tx: &mut sqlx::AnyConnection,
    entity: Entity,
    id: i64,
) -> Result<String, AppError> {
    let sql = format!("SELECT client_uuid FROM {} WHERE id = $1", entity.table());
    let uuid: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    Ok(uuid.expect("every row has carried a client_uuid since migration 0007_sync.sql"))
}

/// The reverse of `uuid_of`: the internal id a `client_uuid` names, for entities where a
/// caller needs to reason about the row as an integer -- as `parent_is_valid` does, since the
/// tree it walks is built entirely out of integer `parent_id`s.
pub async fn id_of(
    tx: &mut sqlx::AnyConnection,
    entity: Entity,
    uuid: &str,
) -> Result<i64, AppError> {
    let sql = format!("SELECT id FROM {} WHERE client_uuid = $1", entity.table());
    let id: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(uuid)
        .fetch_one(&mut *tx)
        .await?;
    Ok(id.expect("every row has carried a client_uuid since migration 0007_sync.sql"))
}

/// Whether `candidate_parent_id` may become `object_id`'s parent: it must exist, belong to
/// `user_id`, and not be deleted; and it must not be `object_id` itself or a descendant of it,
/// which would leave the object as its own ancestor once the write took effect.
///
/// `object_id` is `None` on create, where the object being created has no id yet and therefore
/// cannot possibly be anyone's ancestor -- only existence and ownership are checked there.
///
/// Shared by the REST handler and sync's `Set` handling. A recursive check written twice is a
/// recursive check that can drift into disagreeing twice, which is worse here than for a plain
/// existence check -- this is the one field in the app where the two doors disagreeing could
/// corrupt data rather than merely let a bad value through.
pub async fn parent_is_valid(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    object_id: Option<i64>,
    candidate_parent_id: i64,
) -> Result<bool, AppError> {
    let exists: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM objects WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
    )
    .bind(candidate_parent_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    if exists.is_none() {
        return Ok(false);
    }
    let Some(object_id) = object_id else {
        return Ok(true);
    };

    // Walk the candidate's own ancestor chain (itself, then its parent, then its parent's
    // parent, ...). If `object_id` ever appears in it, making the candidate `object_id`'s
    // parent would close a loop: the candidate is already inside the subtree rooted at
    // `object_id`, including the trivial case where the candidate IS `object_id`.
    //
    // Plain `UNION`, not `UNION ALL`: this function is the only thing standing between a
    // write and an actual cycle, so if one ever got into the data anyway -- a bug, an
    // import, someone editing the database by hand -- `UNION ALL` would walk it forever
    // rather than answer. `UNION`'s deduplication is what makes the recursion terminate on a
    // cycle instead of spinning; the cost is one extra dedup pass on a chain this app expects
    // to be a handful of rows deep at most.
    //
    // `crate::db::Bool`, not a bare `bool`: `EXISTS(...)` decodes as an INTEGER 0/1 on SQLite
    // and a real BOOLEAN on PostgreSQL, and `Decode<Any> for bool` only accepts the latter --
    // the same defect that once broke every `/users` read, here on a query this project has
    // never run before rather than a column it has always had.
    let would_cycle: (crate::db::Bool,) = sqlx::query_as(
        "WITH RECURSIVE ancestors(id) AS ( \
           SELECT id FROM objects WHERE id = $1 \
           UNION \
           SELECT o.parent_id FROM objects o JOIN ancestors a ON o.id = a.id WHERE o.parent_id IS NOT NULL \
         ) SELECT EXISTS (SELECT 1 FROM ancestors WHERE id = $2)")
        .bind(candidate_parent_id).bind(object_id)
        .fetch_one(&mut *tx).await?;
    Ok(!bool::from(would_cycle.0))
}

/// Inserts one `changes` row. Private: every caller goes through `record_create`,
/// `record_update`, `record_delete` or `log_cascade`, which is what keeps `field`/`value`
/// tied to `op` the same way `api::sync::push` ties them (NULL for anything but a `set`).
///
/// `field_value` and `clock` are bundled pairs -- `field` never appears without the `value` it
/// names, and `edited_at` never travels without the `device_id` that clock reading belongs to
/// -- which is also what keeps this under clippy's argument-count limit.
async fn insert_change(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    entity: Entity,
    entity_uuid: &str,
    op: OpKind,
    field_value: Option<(&str, &serde_json::Value)>,
    clock: (&str, &str),
) -> Result<(), AppError> {
    let (field, value) = match field_value {
        Some((field, value)) => (Some(field), Some(value.to_string())),
        None => (None, None),
    };
    let (edited_at, device_id) = clock;
    sqlx::query(
        // `seq` comes from the column's own default -- `AUTOINCREMENT` on SQLite,
        // `GENERATED BY DEFAULT AS IDENTITY` on PostgreSQL -- not from an explicit value here.
        // That default only promises a number that is unique and increasing; it does not
        // promise no gaps, and a rolled-back transaction burns its number on either backend
        // (SQLite's `AUTOINCREMENT` counter lives in `sqlite_sequence`, which does not roll
        // back with the row).
        //
        // What a pull actually depends on is monotonic, commit-order numbering: `seq > cursor`
        // must never revisit a number a device already passed. SQLite gets that for free from
        // having one writer. PostgreSQL does not -- concurrent transactions can commit out of
        // order -- so `db::begin_write` takes an advisory lock around every write transaction
        // that reaches this insert, serialising them. That lock is what a pull's safety rests
        // on; the gaps a rollback leaves behind are cosmetic, and nothing here reads for them.
        "INSERT INTO changes \
         (entity, entity_uuid, op, field, value, edited_at, applied_at, user_id, \
          device_id, client_op_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(entity.as_str())
    .bind(entity_uuid)
    .bind(op.as_str())
    .bind(field)
    .bind(value)
    .bind(edited_at)
    .bind(crate::db::now())
    .bind(user_id)
    .bind(device_id)
    // Fresh per row, exactly as `log_cascade` already does for a cascaded child: nothing
    // a REST write does is idempotency-checked by client_op_id the way a pushed op is (the
    // request itself is the client's only attempt), so there is no id to reuse here.
    //
    // That is also why this insert, unlike the near-identical one in `api::sync::push`,
    // carries no `ON CONFLICT (user_id, client_op_id) DO NOTHING`. There, the client picks
    // the id and can send it twice, so the clause is how the unique index -- rather than a
    // read that can go stale between two statements -- decides whether the op has already
    // been applied. Here the id is minted a line above and cannot collide with anything;
    // the same clause would only be able to swallow a bug that produced one.
    .bind(uuid::Uuid::new_v4().to_string())
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Upserts one `field_clock` row. Shared by `record_create`/`record_update` and by
/// `apply_op`'s `set` handling, which used to carry its own copy of this exact statement.
pub(crate) async fn stamp_field_clock(
    tx: &mut sqlx::AnyConnection,
    entity: Entity,
    entity_uuid: &str,
    field: &str,
    edited_at: &str,
    device_id: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO field_clock (entity, entity_uuid, field, edited_at, device_id) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT(entity, entity_uuid, field) \
         DO UPDATE SET edited_at = excluded.edited_at, device_id = excluded.device_id",
    )
    .bind(entity.as_str())
    .bind(entity_uuid)
    .bind(field)
    .bind(edited_at)
    .bind(device_id)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Logs one `create` op for a REST-created row, and stamps `field_clock` for *every* field in
/// `entity`'s whitelist (`sync::whitelist`), not just the ones the caller happened to mention.
///
/// An INSERT sets every column, including the ones left at their default (an object's
/// `cover_attachment_id`, a reminder's `done_at`) -- so every whitelisted field genuinely was
/// "set" by it, at this moment, to whatever the row now holds. Skipping the ones that landed as
/// NULL would leave them with no clock at all, and a stale offline `set` aimed at one of them --
/// stamped before this row even existed -- would then win last-write-wins by default, because
/// an absent `field_clock` row loses to nothing (see `apply_op`'s `set` handling: no stored row
/// means the incoming op is accepted unconditionally).
pub(crate) async fn record_create(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    entity: Entity,
    entity_uuid: &str,
    edited_at: &str,
) -> Result<(), AppError> {
    insert_change(
        tx,
        user_id,
        entity,
        entity_uuid,
        OpKind::Create,
        None,
        (edited_at, DEVICE_ID),
    )
    .await?;
    for (field, _) in super::whitelist(entity) {
        stamp_field_clock(tx, entity, entity_uuid, field, edited_at, DEVICE_ID).await?;
    }
    Ok(())
}

/// Logs one `set` op per entry in `changed_fields`, and stamps `field_clock` for each.
///
/// `changed_fields` must already be filtered to fields whose value actually differs from what
/// is stored -- this function logs and stamps exactly what it is given, without comparing
/// again. That filtering is the caller's job because only the caller has both the old row and
/// the new one in a shape it can compare; doing it here would mean a second, generic diff
/// against a type this module does not otherwise know. Logging an unchanged field would make
/// the log say something happened that didn't -- a no-op edit did not happen, and the log
/// should not claim it did.
pub(crate) async fn record_update(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    entity: Entity,
    entity_uuid: &str,
    changed_fields: &[(&str, serde_json::Value)],
    edited_at: &str,
) -> Result<(), AppError> {
    for (field, value) in changed_fields {
        insert_change(
            tx,
            user_id,
            entity,
            entity_uuid,
            OpKind::Set,
            Some((field, value)),
            (edited_at, DEVICE_ID),
        )
        .await?;
        stamp_field_clock(tx, entity, entity_uuid, field, edited_at, DEVICE_ID).await?;
    }
    Ok(())
}

/// Logs one `delete` op for `entity_uuid` itself. Cascaded children -- an object's activities,
/// reminders and attachments; an activity's attachments -- are a separate op each, logged by
/// `log_cascade`; this call only ever accounts for the row the caller actually tombstoned.
pub(crate) async fn record_delete(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    entity: Entity,
    entity_uuid: &str,
    edited_at: &str,
) -> Result<(), AppError> {
    insert_change(
        tx,
        user_id,
        entity,
        entity_uuid,
        OpKind::Delete,
        None,
        (edited_at, DEVICE_ID),
    )
    .await
}

/// Tombstones an object's activities, reminders and attachments, and everything inside it --
/// its children, their children, and so on -- answering every row this call actually
/// tombstoned so the caller can log them.
///
/// Called by both `apply_op` (a sync `delete` op) and `api::objects::delete` (a REST delete):
/// the same three tables, and `deleted_at IS NULL` on each so a child tombstoned earlier keeps
/// its original time instead of being restamped by a repeat of the parent's delete -- and so a
/// repeat contributes nothing to the log the second time.
///
/// Attachments are matched on `object_id`, not on their activity, because every attachment
/// carries the object it belongs to whether or not it also names an activity. That is the same
/// column both callers use, so neither can reach a row the other misses.
///
/// The descent is a flat loop over a worklist, not a function that calls itself. A
/// self-calling `async fn`, even boxed with `Box::pin` (which only heap-allocates the future's
/// *state*, not the `poll` call chain), still spends one stack frame per level of the tree.
/// Nothing in the application enforces a depth limit, so a chain built by anyone with an
/// account -- eventually, once anything writes `parent_id` -- could overflow the stack and
/// abort the whole process for every user on the instance. A worklist has no such limit:
/// however deep the tree, this function's own stack usage never grows.
///
/// The worklist holds integer ids, not uuids, for the same reason `nameable` already treats a
/// row's `client_uuid` as possibly absent everywhere else in this file: a child missing one
/// must still be tombstoned and still have ITS OWN children walked -- an id every row carries,
/// unconditionally. Keying the traversal on uuid instead would let a single NULL strand an
/// entire live subtree, walked by nothing, forever.
///
/// The order the worklist produces is breadth-first, and that is load-bearing for the pull
/// feed: a node is dequeued -- and so has its own children discovered, tombstoned and pushed
/// onto `cascaded` -- strictly after the iteration that enqueued it, so a parent's delete is
/// always appended before any of its descendants'. `log_cascade` inserts in that order under
/// the write lock, which is what gives every descendant a higher `changes.seq` than its own
/// ancestor, and a device applying the pull stream in order never sees a child's delete before
/// the parent's.
///
/// `fetch_one` for the root id is safe at both call sites: `apply_op` has already rejected an
/// `entity_uuid` that names no row (as "unknown entity_uuid") before it reaches the delete
/// branch, and the REST handler has already loaded the object it is deleting.
pub(crate) async fn cascade_object(
    tx: &mut sqlx::AnyConnection,
    object_uuid: &str,
    now: &str,
) -> Result<Vec<(Entity, String)>, AppError> {
    let mut cascaded = Vec::new();
    let root_id: i64 = sqlx::query_scalar("SELECT id FROM objects WHERE client_uuid = $1")
        .bind(object_uuid)
        .fetch_one(&mut *tx)
        .await?;
    let mut queue: std::collections::VecDeque<i64> = std::collections::VecDeque::from([root_id]);

    while let Some(current_id) = queue.pop_front() {
        // Of the three cascaded tables, only `activities` carries `updated_at`
        // (migrations/sqlite/0001_init.sql) -- `reminders` and `attachments` don't, so there is
        // nothing to bump on those two.
        for (entity, has_updated_at) in [
            (Entity::Activity, true),
            (Entity::Reminder, false),
            (Entity::Attachment, false),
        ] {
            // The table name comes from `Entity::table` over a closed set fixed above, never
            // from the request, and the id stays a bind parameter -- the audit `AssertSqlSafe`
            // asks the author to have made.
            let table = entity.table();
            // Read the uuids before the update, while `deleted_at IS NULL` still names exactly
            // the rows this cascade is about to claim.
            let select = format!(
                "SELECT client_uuid FROM {table} WHERE deleted_at IS NULL AND object_id = $1"
            );
            let uuids: Vec<Option<String>> = sqlx::query_scalar(sqlx::AssertSqlSafe(select))
                .bind(current_id)
                .fetch_all(&mut *tx)
                .await?;
            let update = if has_updated_at {
                format!(
                    "UPDATE {table} SET deleted_at = $1, updated_at = $2 \
                         WHERE deleted_at IS NULL AND object_id = $3"
                )
            } else {
                format!(
                    "UPDATE {table} SET deleted_at = $1 \
                         WHERE deleted_at IS NULL AND object_id = $2"
                )
            };
            let query = sqlx::query(sqlx::AssertSqlSafe(update)).bind(now);
            let query = if has_updated_at {
                query.bind(now)
            } else {
                query
            };
            query.bind(current_id).execute(&mut *tx).await?;
            cascaded.extend(nameable(uuids).map(|uuid| (entity, uuid)));
        }

        // Direct children of this level: tombstoned in one statement, then queued for their
        // own turn. A child missing a uuid still gets queued by its id -- it is still
        // tombstoned, and its own children are still walked; only its own log entry is lost,
        // the same cost a NULL already has for the three tables above.
        let children: Vec<(i64, Option<String>)> = sqlx::query_as(
            "SELECT id, client_uuid FROM objects WHERE deleted_at IS NULL AND parent_id = $1",
        )
        .bind(current_id)
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE objects SET deleted_at = $1, updated_at = $1 \
             WHERE deleted_at IS NULL AND parent_id = $2",
        )
        .bind(now)
        .bind(current_id)
        .execute(&mut *tx)
        .await?;
        for (child_id, child_uuid) in children {
            if let Some(uuid) = child_uuid {
                cascaded.push((Entity::Object, uuid));
            }
            queue.push_back(child_id);
        }
    }
    Ok(cascaded)
}

/// Clears an object's cover pointer if the attachment just tombstoned is what it was pointing
/// at. Called by both `apply_op` and `api::attachments::delete`. There is no FK on
/// `cover_attachment_id` -- it is a plain INTEGER column -- so this is the only thing standing
/// between a deleted attachment and a stale id sitting in `objects`, and in every sync
/// snapshot, indefinitely.
pub(crate) async fn clear_cover_of(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    attachment_uuid: &str,
    edited_at: &str,
) -> Result<(), AppError> {
    // Which objects are about to change, before they do: the log needs their uuids. Without
    // the log entry a device that pulls later keeps a cover pointing at a row it has been
    // told is gone.
    let cleared: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT client_uuid FROM objects WHERE deleted_at IS NULL \
         AND cover_attachment_id = (SELECT id FROM attachments WHERE client_uuid = $1)",
    )
    .bind(attachment_uuid)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE objects SET cover_attachment_id = NULL WHERE deleted_at IS NULL \
         AND cover_attachment_id = (SELECT id FROM attachments WHERE client_uuid = $1)",
    )
    .bind(attachment_uuid)
    .execute(&mut *tx)
    .await?;
    for uuid in nameable(cleared) {
        record_update(
            tx,
            user_id,
            Entity::Object,
            &uuid,
            &[("cover_attachment_id", serde_json::Value::Null)],
            edited_at,
        )
        .await?;
    }
    Ok(())
}

/// Tombstones an activity's attachments and unhooks the references to it, answering the
/// attachments this call tombstoned.
///
/// Called by both `apply_op` and `api::activities::delete`, including the two statements that
/// are not tombstones: an object's cover and a reminder's `done_activity_id` would otherwise
/// keep pointing at a row the API now reads as absent. `ON DELETE SET NULL` cannot fire for an
/// UPDATE either, so both are written by hand -- and because there is exactly one copy of this
/// function, the two delete paths cannot leave different databases behind for what is meant to
/// be the same op.
pub(crate) async fn cascade_activity(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    activity_uuid: &str,
    now: &str,
    edited_at: &str,
) -> Result<Vec<(Entity, String)>, AppError> {
    // The cover subquery deliberately does not skip tombstoned attachments: an object pointing
    // at one has a stale cover, and clearing it is the point. Both reference cleanups read the
    // rows they are about to change first and log a `set … null` for each, so a device that
    // pulls later learns of them -- they are ordinary field changes, not tombstones, and the
    // delete row alone says nothing about them.
    let cover_cleared: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT client_uuid FROM objects \
         WHERE deleted_at IS NULL AND cover_attachment_id IN (\
           SELECT id FROM attachments \
           WHERE activity_id = (SELECT id FROM activities WHERE client_uuid = $1))",
    )
    .bind(activity_uuid)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE objects SET cover_attachment_id = NULL \
         WHERE deleted_at IS NULL AND cover_attachment_id IN (\
           SELECT id FROM attachments \
           WHERE activity_id = (SELECT id FROM activities WHERE client_uuid = $1))",
    )
    .bind(activity_uuid)
    .execute(&mut *tx)
    .await?;
    for uuid in nameable(cover_cleared) {
        record_update(
            tx,
            user_id,
            Entity::Object,
            &uuid,
            &[("cover_attachment_id", serde_json::Value::Null)],
            edited_at,
        )
        .await?;
    }
    let unlinked: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT client_uuid FROM reminders \
         WHERE deleted_at IS NULL \
         AND done_activity_id = (SELECT id FROM activities WHERE client_uuid = $1)",
    )
    .bind(activity_uuid)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE reminders SET done_activity_id = NULL \
         WHERE deleted_at IS NULL \
         AND done_activity_id = (SELECT id FROM activities WHERE client_uuid = $1)",
    )
    .bind(activity_uuid)
    .execute(&mut *tx)
    .await?;
    for uuid in nameable(unlinked) {
        record_update(
            tx,
            user_id,
            Entity::Reminder,
            &uuid,
            &[("done_activity_id", serde_json::Value::Null)],
            edited_at,
        )
        .await?;
    }

    let uuids: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT client_uuid FROM attachments WHERE deleted_at IS NULL \
         AND activity_id = (SELECT id FROM activities WHERE client_uuid = $1)",
    )
    .bind(activity_uuid)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE attachments SET deleted_at = $1 WHERE deleted_at IS NULL \
         AND activity_id = (SELECT id FROM activities WHERE client_uuid = $2)",
    )
    .bind(now)
    .bind(activity_uuid)
    .execute(&mut *tx)
    .await?;
    Ok(nameable(uuids)
        .map(|uuid| (Entity::Attachment, uuid))
        .collect())
}

/// The cascaded rows the log can actually name.
///
/// `client_uuid` is nullable, so a row written before the sync protocol existed -- or by any
/// writer that never set one -- has nothing a `changes` entry could address. Such a row is
/// still tombstoned; it is only omitted from the log, because an entry naming NULL would be
/// unusable to every device that read it. No client can be holding a copy of a row it was
/// never able to learn the identity of, so nothing is lost by the omission.
fn nameable(uuids: Vec<Option<String>>) -> impl Iterator<Item = String> {
    uuids.into_iter().flatten()
}

/// Records each cascaded tombstone in the change log, so a pulling device learns the child is
/// gone rather than only hearing about its parent.
///
/// A cascaded delete has no op of its own -- the triggering write (a pushed op, or a REST
/// request) names only the parent -- so each entry gets a freshly minted `client_op_id`.
/// Reusing the parent's, decorated, would risk colliding with an id a client had minted itself,
/// and `idx_changes_user_op` would answer that with a constraint failure that throws away the
/// whole write. Idempotency does not depend on these ids: a pushed delete is resolved by the
/// PARENT's client_op_id before `apply_op` is reached, so the cascade never runs twice for one
/// op, and a REST delete has no client_op_id at all. The child's own `deleted_at IS NULL`
/// filter is the second guard, in case a delete arrives under a new id.
///
/// `edited_at` and `device_id` are the parent's: the cascade is that write's edit at that
/// moment, and a child carrying a different clock would compete with the parent's op.
pub(crate) async fn log_cascade(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    edited_at: &str,
    device_id: &str,
    cascaded: &[(Entity, String)],
) -> Result<(), AppError> {
    for (entity, uuid) in cascaded {
        insert_change(
            tx,
            user_id,
            *entity,
            uuid,
            OpKind::Delete,
            None,
            (edited_at, device_id),
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the fix described in `edited_at_now`'s doc comment: this is a unit test, not a
    /// behavioural one, because `edited_at_now` is `pub(crate)` and the regression it guards
    /// is about the VALUE this function returns, not about anything an integration test could
    /// observe through a timing-dependent race against a REST write's real wall clock.
    /// Sampling ~100 calls and asserting at least one does not end in `.000Z` is
    /// deterministic-red under the previous `canonical_edited_at(&crate::db::now())` body
    /// (that routes every call through `SecondsFormat::Secs`, so EVERY sample ends in
    /// `.000Z`, always) and the false-failure probability under the current body is
    /// astronomically small: a genuine millisecond reading lands on `.000` about 1 time in
    /// 1000, so all 100 independent samples doing so at once is roughly (1/1000)^100.
    #[test]
    fn edited_at_now_carries_real_millisecond_precision() {
        let non_floored = (0..100)
            .filter(|_| !edited_at_now().ends_with(".000Z"))
            .count();
        assert!(
            non_floored > 0,
            "edited_at_now() must read the clock at millisecond precision, not just wrap a \
             whole-second reading in the right shape -- see the doc comment on edited_at_now"
        );
    }
}
