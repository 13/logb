//! Applying an incoming op, and the rule that decides whether it may.

/// Whether an incoming edit supersedes the stored one for a field.
///
/// Timestamps are RFC3339 UTC with a fixed number of digits, canonicalized upstream before
/// reaching here, so a lexical comparison is also a chronological one and no parsing is needed.
/// Equal timestamps are broken by `device_id`: the incoming edit wins if its device_id is
/// lexically greater. This tiebreaker remains consistent across all devices because `device_id`
/// is expected to be unique among concurrently active writers, a precondition that is the
/// client's responsibility to maintain.
///
/// If two devices violate this precondition and use the same device_id, a full tie can occur:
/// equal timestamps and identical device_id mean neither op wins in the comparison. The
/// first-arriving op persists in this case. Convergence is still guaranteed because
/// `GET /sync/pull` orders all ops by the server's `changes.seq`, a monotonic sequence that
/// every client observes identically. All participants independently walk this sequence in the
/// same order, so they each independently keep the same first-arriving op in a tie. An op
/// identical to the stored one does not win, so a replay is a no-op rather than a rewrite.
pub fn wins(
    incoming_edited_at: &str,
    incoming_device: &str,
    stored_edited_at: &str,
    stored_device: &str,
) -> bool {
    match incoming_edited_at.cmp(stored_edited_at) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => incoming_device > stored_device,
    }
}

use crate::error::AppError;
use crate::sync::{syncable_field_type, Entity, FieldType, Op, OpKind, Outcome};

/// A client value that has been checked against its column's type and is ready to bind.
#[derive(Debug, PartialEq, Eq)]
enum Binding {
    Null,
    Integer(i64),
    Text(String),
}

/// Checks a value against the type of the column it is headed for, yielding either something
/// bindable or the reason the op is rejected.
///
/// SQLite columns are dynamically typed, and affinity does not convert what it cannot: an
/// INTEGER column keeps the string `"abc"` verbatim as TEXT, and a TEXT column keeps `true` as
/// `'1'`. Nothing complains on the way in, so the damage only appears on the way out -- and it
/// is not cosmetic. Every read decodes the integer columns as `Option<i64>` (`api::activities`,
/// `api::objects`), so a string sitting in one of them fails to decode: `GET
/// /objects/{id}/activities` and `GET /activities/{id}` answer 500 on every request from then
/// on, from one op sent by any authenticated client.
///
/// From then on, because the write also advances `field_clock`. A correction necessarily
/// carries the value's original -- therefore EARLIER -- `edited_at`, so it loses
/// last-write-wins and is answered `superseded`: the corruption locks out its own repair. The
/// milder direction (a bool or a number into a TEXT column) corrupts silently but is
/// unrepairable for the same reason. So the shape is checked here, before the column is
/// written and before the clock moves.
///
/// Null is legal for every field: it is how a client clears a nullable column. Which columns
/// tolerate it is the schema's NOT NULL constraints to answer, not this function's -- a NULL
/// aimed at a NOT NULL column comes back as a constraint violation and is rejected there.
fn binding(
    field: &str,
    field_type: FieldType,
    value: Option<&serde_json::Value>,
) -> Result<Binding, String> {
    use serde_json::Value;
    match (field_type, value) {
        (_, None | Some(Value::Null)) => Ok(Binding::Null),
        // `as_i64` answers `None` for a float or a magnitude past i64 -- values SQLite would
        // store as a float or as text rather than refuse.
        (FieldType::Integer, Some(Value::Number(n))) => n
            .as_i64()
            .map(Binding::Integer)
            .ok_or_else(|| format!("{field} must be an integer")),
        (FieldType::Integer, Some(_)) => Err(format!("{field} must be an integer")),
        (FieldType::Text, Some(Value::String(s))) => Ok(Binding::Text(s.clone())),
        (FieldType::Text, Some(_)) => Err(format!("{field} must be a string")),
    }
}

/// Checks a bound value against the same rules the matching REST handler enforces on the same
/// column (`ObjectInput::validate`, `ActivityInput::validate`, `ReminderInput::validate` in
/// `api::objects`/`activities`/`reminders`), so a `set` op cannot write anything a REST `PATCH`
/// would refuse with 400. `binding` has already settled the value's SHAPE -- integer vs. text --
/// which says nothing about whether the value itself makes sense: `name = ""`,
/// `purchase_price_cents = -999` and `purchase_date = "not-a-date"` are all shaped correctly and
/// would sail through `binding` untouched. Only a handful of columns happen to carry a SQLite
/// CHECK that catches this by accident (`counter_unit`, `fuel_unit`, `activities.category`,
/// `attachments.kind`); everything else has nothing standing between a client and the row
/// without this.
///
/// Null is always left alone: it means "clear the field", exactly as `binding` already treats
/// it, and whether a given column tolerates it is the schema's NOT NULL constraint to answer.
fn validate_value(entity: Entity, field: &str, bound: &Binding) -> Result<(), String> {
    let Binding::Text(text) = bound else {
        let Binding::Integer(n) = bound else { return Ok(()) };
        // `repeat_months`/`repeat_counter` must be strictly positive -- see
        // `ReminderInput::validate` -- which is a stricter bound than "non-negative" and
        // therefore satisfies it too.
        let must_be_positive =
            matches!((entity, field), (Entity::Reminder, "repeat_months" | "repeat_counter"));
        let non_negative = matches!(
            (entity, field),
            (Entity::Object, "purchase_price_cents")
                | (Entity::Activity, "cost_cents" | "counter_value" | "quantity_milli")
                | (Entity::Reminder, "due_counter")
        );
        if must_be_positive && *n <= 0 {
            return Err(format!("{field} must be > 0"));
        }
        if non_negative && *n < 0 {
            return Err(format!("{field} must be >= 0"));
        }
        return Ok(());
    };

    match (entity, field) {
        (Entity::Object, "name" | "category")
        | (Entity::Activity, "title")
        | (Entity::Reminder, "title") => {
            if text.trim().is_empty() {
                return Err(format!("{field} is required"));
            }
        }
        (Entity::Object, "purchase_date")
        | (Entity::Activity, "date")
        | (Entity::Reminder, "due_date") => {
            // Reuses `objects::validate_date` rather than re-implementing the format, so the
            // two paths cannot drift into accepting different dates.
            crate::api::objects::validate_date(text).map_err(|e| e.to_string())?;
        }
        _ => {}
    }
    Ok(())
}

/// Rewrites a client-supplied timestamp into the one canonical form `wins` can compare.
///
/// `wins` compares `edited_at` lexically, which is only chronological when every value has the
/// same width and the same zone spelling. `db::now()` guarantees that for values the server
/// writes, but `edited_at` arrives from a device and nothing constrains what it sends:
/// `2026-01-01T00:00:00Z` sorts AFTER `2026-01-01T00:00:00.500Z` (`Z` is 0x5A, `.` is 0x2E)
/// while being half a second earlier, and an offset form like `+00:00` does not order against
/// `Z` at all. Either would hand the wrong edit the win, silently and unreproducibly. So every
/// timestamp is parsed and re-emitted as UTC with fixed millisecond precision before it is
/// compared with, or stored beside, any other.
pub fn canonical_edited_at(raw: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc).to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// Whether a database error is a constraint violation, i.e. the statement was refused for what
/// it tried to store rather than because anything is wrong with the database.
///
/// SQLite reports every constraint failure with `SQLITE_CONSTRAINT` (19) as the primary result
/// code in the low byte of the extended code it hands back: 1299 NOT NULL, 275 CHECK, 787
/// FOREIGN KEY, 2067 UNIQUE, and the rest. Testing the low byte therefore catches every subtype,
/// including the ones `DatabaseError::kind()` folds into `ErrorKind::Other`, while still letting
/// an I/O error, a locked database or a schema fault through as the genuine 500 it is.
fn is_constraint_violation(err: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db) = err else {
        return false;
    };
    db.code()
        .and_then(|code| code.parse::<i32>().ok())
        .is_some_and(|code| code & 0xff == SQLITE_CONSTRAINT)
}

/// SQLite's primary result code for a constraint violation.
const SQLITE_CONSTRAINT: i32 = 19;

/// Applies one op inside the caller's transaction and returns how it landed.
///
/// Ownership is resolved by joining back to `objects.user_id` rather than trusting anything in
/// the op, so a uuid belonging to another account cannot be written through.
pub async fn apply_op(
    tx: &mut sqlx::SqliteConnection,
    user_id: i64,
    op: &Op,
) -> Result<Outcome, AppError> {
    if op.entity_uuid.is_empty() || op.device_id.is_empty() {
        return Ok(Outcome::Rejected { reason: "entity_uuid and device_id are required".into() });
    }

    // Does this uuid exist, and does it belong to the caller?
    let owner: Option<i64> = match op.entity {
        Entity::Object => sqlx::query_scalar(
            "SELECT user_id FROM objects WHERE client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::Activity => sqlx::query_scalar(
            "SELECT o.user_id FROM activities a JOIN objects o ON o.id = a.object_id \
             WHERE a.client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::Reminder => sqlx::query_scalar(
            "SELECT o.user_id FROM reminders r JOIN objects o ON o.id = r.object_id \
             WHERE r.client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::Attachment => sqlx::query_scalar(
            "SELECT o.user_id FROM attachments t JOIN objects o ON o.id = t.object_id \
             WHERE t.client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::File => sqlx::query_scalar(
            "SELECT user_id FROM files WHERE client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
    };
    match owner {
        None => return Ok(Outcome::Rejected { reason: "unknown entity_uuid".into() }),
        Some(owner) if owner != user_id => {
            return Ok(Outcome::Rejected { reason: "unknown entity_uuid".into() })
        }
        Some(_) => {}
    }

    match op.op {
        // A create arriving through sync is a client announcing a row it already made; the row
        // itself is inserted by the ordinary REST create, which the client still calls. Here it
        // only has to be logged, so the pull feed carries it to other devices.
        OpKind::Create => Ok(Outcome::Accepted),

        // A delete over sync must leave the same database behind as the same delete over REST.
        // It is a tombstone rather than a row removal, so the schema's `ON DELETE CASCADE`
        // never fires and the cascade has to be spelled out here, exactly as
        // `api::objects::delete` and `api::activities::delete` spell it out.
        //
        // Skipping it was not merely untidy. The children stayed LIVE, and the retention purge
        // later hard-deletes the expired parent -- a real `DELETE`, which does fire the
        // cascade, destroying rows that never got a tombstone and never got a `changes` entry.
        // No device could ever learn they had existed or vanished, and an attachment destroyed
        // that way stranded its `files` row (the purge's pinned list is built from TOMBSTONED
        // attachments, so a live one is never a candidate) and leaked its blob on disk forever.
        //
        // One timestamp for the parent and all its children, and the caller's transaction for
        // all of it, so a half-applied cascade cannot survive a failure part way through.
        OpKind::Delete => {
            // A file is content-addressed and shared by every attachment that references it --
            // "a file dies when its last attachment does" is the model `whitelist` already
            // states, and `purge_orphan_files` is what implements it. Tombstoning one directly
            // here would break that silently, in three separate places at once: nothing else
            // in the codebase ever sets `files.deleted_at`, so the purge's `guards` array (see
            // `sync::feed`) has no `files` entry and would never reclaim the tombstone; bootstrap
            // filters `deleted_at IS NULL`, so the file vanishes from every device's snapshot
            // while its still-live attachment keeps pointing at it; and the upload dedup
            // (`api::attachments::create`) has no `deleted_at` filter, so re-uploading the same
            // bytes would re-adopt the dead row and make the replacement photo unrenderable
            // too. Refusing the op here is what keeps all three of those actually true.
            if op.entity == Entity::File {
                return Ok(Outcome::Rejected {
                    reason: "files are not deletable over sync".into(),
                });
            }

            let now = crate::db::now();
            // Only `objects` and `activities` carry `updated_at` (migrations/0001_init.sql);
            // `reminders`, `attachments` and `files` do not. The REST delete handlers stamp it
            // alongside `deleted_at` wherever the column exists, so this has to too, or a row
            // tombstoned over sync keeps whatever `updated_at` it had before the delete.
            let has_updated_at = matches!(op.entity, Entity::Object | Entity::Activity);
            let sql = if has_updated_at {
                format!(
                    "UPDATE {} SET deleted_at = ?, updated_at = ? \
                     WHERE client_uuid = ? AND deleted_at IS NULL",
                    op.entity.table()
                )
            } else {
                format!(
                    "UPDATE {} SET deleted_at = ? WHERE client_uuid = ? AND deleted_at IS NULL",
                    op.entity.table()
                )
            };
            let query = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(&now);
            let query = if has_updated_at { query.bind(&now) } else { query };
            query.bind(&op.entity_uuid).execute(&mut *tx).await?;

            let cascaded = match op.entity {
                Entity::Object => cascade_object(&mut *tx, &op.entity_uuid, &now).await?,
                Entity::Activity => cascade_activity(&mut *tx, &op.entity_uuid, &now).await?,
                // An attachment has no children to tombstone, but it is not a leaf reference-wise:
                // it can be an object's cover, and `cover_attachment_id` is a plain INTEGER with
                // no FK to enforce that by itself -- see `clear_cover_of`.
                Entity::Attachment => {
                    clear_cover_of(&mut *tx, &op.entity_uuid).await?;
                    Vec::new()
                }
                // A reminder has no children of its own, and nothing else keeps a stray
                // reference to it that a delete would need to clean up.
                Entity::Reminder => Vec::new(),
                Entity::File => unreachable!("a file delete is refused above, before reaching this match"),
            };
            log_cascade(&mut *tx, user_id, op, &cascaded).await?;
            Ok(Outcome::Accepted)
        }

        OpKind::Set => {
            let Some(field) = op.field.as_deref() else {
                return Ok(Outcome::Rejected { reason: "set requires a field".into() });
            };
            let Some(field_type) = syncable_field_type(op.entity, field) else {
                return Ok(Outcome::Rejected { reason: format!("{field} is not settable") });
            };

            // The value has to match the column before anything else looks at it: see
            // `binding` for why a mistyped write is unrepairable rather than merely wrong.
            let bound = match binding(field, field_type, op.value.as_ref()) {
                Ok(bound) => bound,
                Err(reason) => return Ok(Outcome::Rejected { reason }),
            };

            // The shape is right, but nothing yet says the VALUE makes sense -- see
            // `validate_value` for why that is a real gap and not paranoia.
            if let Err(reason) = validate_value(op.entity, field, &bound) {
                return Ok(Outcome::Rejected { reason });
            }

            // The whitelist lets `kind` change freely, but `objects::update` refuses to point
            // `cover_attachment_id` at anything but a live `photo` attachment (`AND kind =
            // 'photo'`), and the derived `cover_file_id` never re-checks `kind` on read. Without
            // this, `set attachment.kind = document` could turn a live cover into a document
            // over sync and leave the pointer live but meaningless -- exactly what the REST
            // guard exists to prevent, reached through the second door.
            if op.entity == Entity::Attachment && field == "kind" {
                if let Binding::Text(new_kind) = &bound {
                    if new_kind != "photo" {
                        let is_cover: Option<(i64,)> = sqlx::query_as(
                            "SELECT o.id FROM objects o \
                             JOIN attachments a ON a.id = o.cover_attachment_id \
                             WHERE a.client_uuid = ? AND o.deleted_at IS NULL")
                            .bind(&op.entity_uuid)
                            .fetch_optional(&mut *tx).await?;
                        if is_cover.is_some() {
                            return Ok(Outcome::Rejected {
                                reason: "kind cannot change away from photo while it is an \
                                         object's cover".into(),
                            });
                        }
                    }
                }
            }

            // Two whitelisted fields are foreign keys, and a check on the field NAME says
            // nothing about the VALUE. Without this, `set object.cover_attachment_id` could
            // point a row at another account's attachment -- which the object read then hands
            // back as `cover_file_id`. The REST handlers already refuse a cross-object
            // reference; sync has to refuse it identically, or it is just a second, unguarded
            // door onto the same write.
            //
            // Only an integer can name a row, and `binding` has already refused everything
            // else these two INTEGER fields could have carried. Null falls through untouched:
            // clearing the reference is how a client removes a cover.
            if let Binding::Integer(referenced) = &bound {
                let permitted: Option<i64> = match (op.entity, field) {
                    (Entity::Object, "cover_attachment_id") => sqlx::query_scalar(
                        "SELECT a.id FROM attachments a JOIN objects o ON o.id = a.object_id \
                         WHERE a.id = ? AND o.client_uuid = ? AND a.deleted_at IS NULL")
                        .bind(referenced).bind(&op.entity_uuid)
                        .fetch_optional(&mut *tx).await?,
                    (Entity::Reminder, "done_activity_id") => sqlx::query_scalar(
                        "SELECT act.id FROM activities act \
                         JOIN reminders r ON r.object_id = act.object_id \
                         WHERE act.id = ? AND r.client_uuid = ? AND act.deleted_at IS NULL")
                        .bind(referenced).bind(&op.entity_uuid)
                        .fetch_optional(&mut *tx).await?,
                    _ => Some(*referenced),
                };
                if permitted.is_none() {
                    return Ok(Outcome::Rejected {
                        reason: format!("{field} must reference a row on the same object"),
                    });
                }
            }

            let stored: Option<(String, String)> = sqlx::query_as(
                "SELECT edited_at, device_id FROM field_clock \
                 WHERE entity = ? AND entity_uuid = ? AND field = ?")
                .bind(op.entity.as_str()).bind(&op.entity_uuid).bind(field)
                .fetch_optional(&mut *tx).await?;

            if let Some((stored_at, stored_device)) = &stored {
                if !wins(&op.edited_at, &op.device_id, stored_at, stored_device) {
                    return Ok(Outcome::Superseded);
                }
            }

            // `field` and the table name are both from closed sets (the whitelist and
            // `Entity::table`), never from the request, so this interpolation cannot be
            // steered by a caller. The value stays a bind parameter. That closed-set argument
            // is exactly what `AssertSqlSafe` asks the author to have made before sqlx will
            // take a `String` as SQL.
            let sql = format!(
                "UPDATE {} SET {field} = ? WHERE client_uuid = ?",
                op.entity.table()
            );
            let query = sqlx::query(sqlx::AssertSqlSafe(sql));
            // Nothing decides here: `binding` already settled what may reach the column, so
            // there is no second, weaker opinion about types for the first to drift from.
            let query = match bound {
                Binding::Null => query.bind(None::<String>),
                Binding::Integer(n) => query.bind(n),
                Binding::Text(s) => query.bind(s),
            };
            // A value that the schema refuses -- NULL into a NOT NULL column, a string outside
            // a CHECK list -- is a malformed op, and the contract puts a malformed op in the
            // `rejected` bucket. Letting the `sqlx::Error` escape instead made it a 500, which
            // rolled back the whole batch including ops already accepted, and the client's
            // identical retry hit the same op and the same 500 forever: sync stalled on one op
            // the client had no way to identify. Only constraint failures are converted; every
            // other database error is a genuine fault and still propagates.
            //
            // This relies on SQLite's default `ON CONFLICT ABORT`, which rolls back only the
            // failing statement and leaves the enclosing transaction open and usable -- so the
            // ops either side of a rejected one still commit together, and batch atomicity
            // holds. `a_constraint_violating_op_is_rejected_without_poisoning_the_batch` in
            // `tests/sync.rs` pins that, asserting the writes before and after really landed.
            if let Err(e) = query.bind(&op.entity_uuid).execute(&mut *tx).await {
                if is_constraint_violation(&e) {
                    return Ok(Outcome::Rejected {
                        reason: format!("{field} violates a database constraint"),
                    });
                }
                return Err(e.into());
            }

            sqlx::query(
                "INSERT INTO field_clock (entity, entity_uuid, field, edited_at, device_id) \
                 VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(entity, entity_uuid, field) \
                 DO UPDATE SET edited_at = excluded.edited_at, device_id = excluded.device_id")
                .bind(op.entity.as_str()).bind(&op.entity_uuid).bind(field)
                .bind(&op.edited_at).bind(&op.device_id)
                .execute(&mut *tx).await?;

            Ok(Outcome::Accepted)
        }
    }
}

/// Tombstones an object's activities, reminders and attachments, answering the children this
/// call actually tombstoned so the caller can log them.
///
/// Mirrors `api::objects::delete`: the same three tables, and `deleted_at IS NULL` on each so a
/// child tombstoned earlier keeps its original time instead of being restamped by a repeat of
/// the parent's delete -- and so a repeat contributes nothing to the log the second time.
///
/// Attachments are matched on `object_id`, not on their activity, because every attachment
/// carries the object it belongs to whether or not it also names an activity. That is the same
/// column the REST handler uses, so neither path can reach a row the other misses.
async fn cascade_object(
    tx: &mut sqlx::SqliteConnection,
    object_uuid: &str,
    now: &str,
) -> Result<Vec<(Entity, String)>, AppError> {
    let mut cascaded = Vec::new();
    // Of the three cascaded tables, only `activities` carries `updated_at`
    // (migrations/0001_init.sql) -- `reminders` and `attachments` don't, so there is nothing to
    // bump on those two.
    for (entity, has_updated_at) in
        [(Entity::Activity, true), (Entity::Reminder, false), (Entity::Attachment, false)]
    {
        // The table name comes from `Entity::table` over a closed set fixed above, never from
        // the request, and the uuid stays a bind parameter -- the audit `AssertSqlSafe` asks
        // the author to have made.
        let table = entity.table();
        const MINE: &str =
            "deleted_at IS NULL AND object_id = (SELECT id FROM objects WHERE client_uuid = ?)";
        // Read the uuids before the update, while `deleted_at IS NULL` still names exactly the
        // rows this cascade is about to claim.
        let select = format!("SELECT client_uuid FROM {table} WHERE {MINE}");
        let uuids: Vec<Option<String>> = sqlx::query_scalar(sqlx::AssertSqlSafe(select))
            .bind(object_uuid).fetch_all(&mut *tx).await?;
        let update = if has_updated_at {
            format!("UPDATE {table} SET deleted_at = ?, updated_at = ? WHERE {MINE}")
        } else {
            format!("UPDATE {table} SET deleted_at = ? WHERE {MINE}")
        };
        let query = sqlx::query(sqlx::AssertSqlSafe(update)).bind(now);
        let query = if has_updated_at { query.bind(now) } else { query };
        query.bind(object_uuid).execute(&mut *tx).await?;
        cascaded.extend(nameable(uuids).map(|uuid| (entity, uuid)));
    }
    Ok(cascaded)
}

/// Clears an object's cover pointer if the attachment just tombstoned is what it was pointing
/// at, mirroring `api::attachments::delete`. There is no FK on `cover_attachment_id` -- it is a
/// plain INTEGER column -- so this is the only thing standing between a deleted attachment and
/// a stale id sitting in `objects`, and in every sync snapshot, indefinitely.
async fn clear_cover_of(
    tx: &mut sqlx::SqliteConnection,
    attachment_uuid: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE objects SET cover_attachment_id = NULL WHERE deleted_at IS NULL \
         AND cover_attachment_id = (SELECT id FROM attachments WHERE client_uuid = ?)")
        .bind(attachment_uuid).execute(&mut *tx).await?;
    Ok(())
}

/// Tombstones an activity's attachments and unhooks the references to it, answering the
/// attachments this call tombstoned.
///
/// Mirrors `api::activities::delete`, including the two statements that are not tombstones:
/// an object's cover and a reminder's `done_activity_id` would otherwise keep pointing at rows
/// the API now reads as absent. `ON DELETE SET NULL` cannot fire for an UPDATE either, so both
/// are written by hand -- and if only the REST path did so, the two delete paths would leave
/// different databases behind for the same op.
async fn cascade_activity(
    tx: &mut sqlx::SqliteConnection,
    activity_uuid: &str,
    now: &str,
) -> Result<Vec<(Entity, String)>, AppError> {
    // As in the REST handler, the cover subquery deliberately does not skip tombstoned
    // attachments: an object pointing at one has a stale cover, and clearing it is the point.
    sqlx::query(
        "UPDATE objects SET cover_attachment_id = NULL \
         WHERE deleted_at IS NULL AND cover_attachment_id IN (\
           SELECT id FROM attachments \
           WHERE activity_id = (SELECT id FROM activities WHERE client_uuid = ?))")
        .bind(activity_uuid).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE reminders SET done_activity_id = NULL \
         WHERE deleted_at IS NULL \
         AND done_activity_id = (SELECT id FROM activities WHERE client_uuid = ?)")
        .bind(activity_uuid).execute(&mut *tx).await?;

    let uuids: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT client_uuid FROM attachments WHERE deleted_at IS NULL \
         AND activity_id = (SELECT id FROM activities WHERE client_uuid = ?)")
        .bind(activity_uuid).fetch_all(&mut *tx).await?;
    sqlx::query(
        "UPDATE attachments SET deleted_at = ? WHERE deleted_at IS NULL \
         AND activity_id = (SELECT id FROM activities WHERE client_uuid = ?)")
        .bind(now).bind(activity_uuid).execute(&mut *tx).await?;
    Ok(nameable(uuids).map(|uuid| (Entity::Attachment, uuid)).collect())
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
/// A cascaded delete has no op of its own -- the client sent one op, for the parent -- so each
/// entry gets a freshly minted `client_op_id`. Reusing the parent's, decorated, would risk
/// colliding with an id a client had minted itself, and `idx_changes_user_op` would answer
/// that with a constraint failure that throws away the whole batch. Idempotency does not
/// depend on these ids: the push handler resolves a replayed op by the PARENT's id before
/// `apply_op` is reached, so the cascade never runs twice for one op. The child's own
/// `deleted_at IS NULL` filter is the second guard, in case a delete arrives under a new id.
///
/// `edited_at` and `device_id` are the parent's: the cascade is that device's edit at that
/// moment, and a child carrying a different clock would compete with the parent's op.
async fn log_cascade(
    tx: &mut sqlx::SqliteConnection,
    user_id: i64,
    op: &Op,
    cascaded: &[(Entity, String)],
) -> Result<(), AppError> {
    for (entity, uuid) in cascaded {
        sqlx::query(
            "INSERT INTO changes \
             (entity, entity_uuid, op, field, value, edited_at, applied_at, user_id, \
              device_id, client_op_id) \
             VALUES (?, ?, 'delete', NULL, NULL, ?, ?, ?, ?, ?)")
            .bind(entity.as_str())
            .bind(uuid)
            .bind(&op.edited_at)
            .bind(crate::db::now())
            .bind(user_id)
            .bind(&op.device_id)
            .bind(uuid::Uuid::new_v4().to_string())
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_edit_wins() {
        assert!(wins("2026-01-02T00:00:00Z", "phone", "2026-01-01T00:00:00Z", "desktop"));
    }

    #[test]
    fn an_older_edit_loses() {
        assert!(!wins("2026-01-01T00:00:00Z", "phone", "2026-01-02T00:00:00Z", "desktop"));
    }

    #[test]
    fn a_tie_breaks_on_device_id_with_the_greater_winning() {
        let t = "2026-01-01T00:00:00Z";
        assert!(wins(t, "phone", t, "desktop"), "phone > desktop");
        assert!(!wins(t, "desktop", t, "phone"), "desktop < phone");
    }

    #[test]
    fn a_value_of_the_wrong_shape_never_reaches_the_column() {
        use serde_json::json;
        // An integer column takes an integer or null, and nothing else -- a string is what
        // SQLite would have stored as TEXT, making every later read of the row fail to decode.
        assert_eq!(binding("counter_value", FieldType::Integer, Some(&json!(7))), Ok(Binding::Integer(7)));
        assert_eq!(binding("counter_value", FieldType::Integer, None), Ok(Binding::Null));
        assert_eq!(binding("counter_value", FieldType::Integer, Some(&json!(null))), Ok(Binding::Null));
        for bad in [json!("abc"), json!(true), json!(1.5), json!([1]), json!({})] {
            assert_eq!(
                binding("counter_value", FieldType::Integer, Some(&bad)),
                Err("counter_value must be an integer".into()),
                "{bad} is not an integer"
            );
        }

        // And the other direction: `true` in a TEXT column would have been stored as '1'.
        assert_eq!(binding("name", FieldType::Text, Some(&json!("Golf"))), Ok(Binding::Text("Golf".into())));
        assert_eq!(binding("name", FieldType::Text, Some(&json!(null))), Ok(Binding::Null));
        for bad in [json!(true), json!(7), json!(1.5), json!([1]), json!({})] {
            assert_eq!(
                binding("name", FieldType::Text, Some(&bad)),
                Err("name must be a string".into()),
                "{bad} is not a string"
            );
        }
    }

    #[test]
    fn a_replay_of_the_same_op_does_not_win() {
        let t = "2026-01-01T00:00:00Z";
        assert!(!wins(t, "phone", t, "phone"), "identical edit is not newer than itself");
    }
}
