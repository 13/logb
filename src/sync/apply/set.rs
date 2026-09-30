//! A `set` op: one field of one row, checked, then written if it wins last-write-wins.

use crate::api::objects::PARENT_REJECTION;
use crate::error::AppError;
use crate::sync::record;
use crate::sync::{syncable_field_type, Entity, FieldType, Op, Outcome};
use super::activity::check_activity;
use super::object::check_object;
use super::reminder::{revalidate_reminder, REVALIDATED_REMINDER_FIELDS};
use super::types::type_field;
use super::value::{binding, canonical_place, is_place, is_tags, validate_value, Binding};
use super::{canonical_tags, is_constraint_violation, wins};

/// Writes one field of a live row the caller owns, after every check the REST door would make.
pub(super) async fn apply_set(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    op: &Op,
) -> Result<Outcome, AppError> {
    // A row already tombstoned (by a delete this same device raced with, or one that
    // reached the server first from another device) refuses every `set` outright, and
    // has to be the very first thing checked -- before the op's field is even looked
    // at. Every check below it (the field whitelist, `binding`, `validate_value`, tags
    // normalisation, `type_field`'s own-name-taken check, the FK checks) can fail for
    // reasons that have nothing to do with deletion, and a client renaming a deleted
    // type to a name a DIFFERENT, live type now holds must not be told "you already
    // have a type with this name" -- it must be told the row it named is gone. Ownership
    // is already settled above (the `owner` match), so this only has to ask about
    // `deleted_at`, and it asks before `field_clock` is anywhere near read: rejecting a
    // deleted row is not a race last-write-wins decides, so it does not need
    // `field_clock`'s serialisation and a rejected op must not advance the clock or add
    // a `changes` row regardless.
    let deleted_at: Option<(Option<String>,)> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT deleted_at FROM {} WHERE client_uuid = $1",
            op.entity.table()
        )))
        .bind(&op.entity_uuid)
        .fetch_optional(&mut *tx)
        .await?;
    if deleted_at.and_then(|(d,)| d).is_some() {
        return Ok(Outcome::Rejected {
            reason: "this item was deleted".into(),
        });
    }

    let Some(field) = op.field.as_deref() else {
        return Ok(Outcome::Rejected {
            reason: "set requires a field".into(),
        });
    };
    let Some(field_type) = syncable_field_type(op.entity, field) else {
        return Ok(Outcome::Rejected {
            reason: format!("{field} is not settable"),
        });
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

    if op.entity == Entity::Reminder && REVALIDATED_REMINDER_FIELDS.contains(&field) {
        if let Some(reason) =
            revalidate_reminder(&mut *tx, &op.entity_uuid, field, op.value.as_ref()).await?
        {
            return Ok(Outcome::Rejected { reason });
        }
    }

    // Tags are normalised, not merely checked: a REST write stores the normalised
    // spelling, so a sync write has to store the same one. Done again here, even though
    // the push handler already rewrote the logged value, so `apply_op` never relies on
    // its caller for what reaches the column. A NULL falls through to the NOT NULL
    // constraint and is rejected there.
    let bound = match bound {
        Binding::Text(text) if is_tags(op.entity, field) => match canonical_tags(&text) {
            Ok(normalised) => Binding::Text(normalised),
            Err(reason) => return Ok(Outcome::Rejected { reason }),
        },
        // A trip place is trimmed, not merely checked, for the same reason tags are --
        // done again here even though the push handler already rewrote the logged
        // value (`canonical_value`), so `apply_op` never relies on its caller for what
        // reaches the column.
        Binding::Text(text) if is_place(op.entity, field) => {
            canonical_place(&text).map_or(Binding::Null, Binding::Text)
        }
        other => other,
    };

    match check_activity(&mut *tx, op, field, &bound).await? {
        Outcome::Accepted => {}
        rejected => return Ok(rejected),
    }
    match check_object(&mut *tx, user_id, op, field, &bound).await? {
        Outcome::Accepted => {}
        rejected => return Ok(rejected),
    }
    let bound = if op.entity == Entity::ObjectType {
        match type_field(&mut *tx, user_id, &op.entity_uuid, field, bound).await? {
            Ok(bound) => bound,
            Err(reason) => return Ok(Outcome::Rejected { reason }),
        }
    } else {
        bound
    };

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
                     WHERE a.client_uuid = $1 AND o.deleted_at IS NULL",
                )
                .bind(&op.entity_uuid)
                .fetch_optional(&mut *tx)
                .await?;
                if is_cover.is_some() {
                    return Ok(Outcome::Rejected {
                        reason: "kind cannot change away from photo while it is an \
                                 object's cover"
                            .into(),
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
            (Entity::Object, "cover_attachment_id") => {
                sqlx::query_scalar(
                    "SELECT a.id FROM attachments a JOIN objects o ON o.id = a.object_id \
                 WHERE a.id = $1 AND o.client_uuid = $2 AND a.deleted_at IS NULL",
                )
                .bind(referenced)
                .bind(&op.entity_uuid)
                .fetch_optional(&mut *tx)
                .await?
            }
            // A parent is not "a row on the same object" but a row on the same
            // ACCOUNT that must also not be inside this object's own subtree, so it
            // goes through `record::parent_is_valid` -- the single copy of that rule
            // the REST door uses too.
            (Entity::Object, "parent_id") => {
                let object_id =
                    record::id_of(&mut *tx, Entity::Object, &op.entity_uuid).await?;
                if record::parent_is_valid(&mut *tx, user_id, Some(object_id), *referenced)
                    .await?
                {
                    Some(*referenced)
                } else {
                    None
                }
            }
            (Entity::Reminder, "done_activity_id") => {
                sqlx::query_scalar(
                    "SELECT act.id FROM activities act \
                 JOIN reminders r ON r.object_id = act.object_id \
                 WHERE act.id = $1 AND r.client_uuid = $2 AND act.deleted_at IS NULL",
                )
                .bind(referenced)
                .bind(&op.entity_uuid)
                .fetch_optional(&mut *tx)
                .await?
            }
            _ => Some(*referenced),
        };
        if permitted.is_none() {
            // `parent_id` gets its own sentence, word for word the one
            // `objects::update` answers a bad parent with: the two doors refuse the
            // same write for the same stated reason rather than leaving a client to
            // guess why only one of them complained about "the same object".
            let reason = match (op.entity, field) {
                (Entity::Object, "parent_id") => PARENT_REJECTION.to_string(),
                _ => format!("{field} must reference a row on the same object"),
            };
            return Ok(Outcome::Rejected { reason });
        }
    }

    // Read, decide with `wins`, then write: three statements that have to behave as
    // one, because whatever else could write this row between the read and the write
    // could make its own stale write disappear behind the very check meant to stop it.
    // That is only safe because writers are serialised -- `apply_op` never runs outside
    // a transaction `db::begin_write` opened, and on PostgreSQL that call takes the
    // advisory lock (`Backend::write_lock`) for the transaction's whole lifetime, so no
    // other write transaction's `field_clock` read or write can land between this read
    // and the `UPDATE` below. Remove that lock and this exact read-compare-write loses:
    // `tests/it/concurrency.rs`'s `the_later_edit_wins_regardless_of_arrival_order` fails
    // on PostgreSQL without it (and stays green on SQLite, whose own `BEGIN IMMEDIATE`
    // already serialises writers). Do not add a lock here -- the one this depends on is
    // already held for the whole push.
    let stored: Option<(String, String)> = sqlx::query_as(
        "SELECT edited_at, device_id FROM field_clock \
         WHERE entity = $1 AND entity_uuid = $2 AND field = $3",
    )
    .bind(op.entity.as_str())
    .bind(&op.entity_uuid)
    .bind(field)
    .fetch_optional(&mut *tx)
    .await?;

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
        "UPDATE {} SET {field} = $1 WHERE client_uuid = $2",
        op.entity.table()
    );
    let query = sqlx::query(sqlx::AssertSqlSafe(sql));
    // Nothing decides here: `binding` already settled what may reach the column, so
    // there is no second, weaker opinion about types for the first to drift from.
    //
    // A NULL still has to be bound with the column's own type. SQLite does not care --
    // every parameter is dynamically typed -- but PostgreSQL infers the parameter's
    // type from what is bound and then refuses `NULL::text` for a `bigint` column, so
    // an untyped `None::<String>` made "clear this reference" a 500 on every integer
    // field. `field_type` is the same source `binding` consulted, so the two cannot
    // drift apart.
    let query = match bound {
        Binding::Null => match field_type {
            FieldType::Integer => query.bind(None::<i64>),
            FieldType::Text => query.bind(None::<String>),
        },
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
    // The savepoint is what makes that survivable on both backends. SQLite's default
    // `ON CONFLICT ABORT` rolls back only the failing statement and leaves the
    // enclosing transaction usable, so this used to run bare; PostgreSQL aborts the
    // whole transaction on any error, and every statement after it -- including the
    // ops already accepted in this batch and the COMMIT -- fails with "current
    // transaction is aborted". Rolling back to a savepoint is the one spelling both
    // understand, and it gives SQLite exactly the statement-level rollback it already
    // had. `a_constraint_violating_op_is_rejected_without_poisoning_the_batch` in
    // `tests/it/sync/` pins that, asserting the writes before and after really landed.
    //
    // The name is a literal, and one `set` op is never nested inside another, so a
    // single name cannot collide with itself.
    sqlx::query("SAVEPOINT logb_set_op")
        .execute(&mut *tx)
        .await?;
    match query.bind(&op.entity_uuid).execute(&mut *tx).await {
        Ok(_) => {
            sqlx::query("RELEASE SAVEPOINT logb_set_op")
                .execute(&mut *tx)
                .await?;
        }
        Err(e) if is_constraint_violation(&e) => {
            sqlx::query("ROLLBACK TO SAVEPOINT logb_set_op")
                .execute(&mut *tx)
                .await?;
            return Ok(Outcome::Rejected {
                reason: format!("{field} violates a database constraint"),
            });
        }
        // Not a constraint failure: a genuine fault, and the whole batch is rolled
        // back with it, so the savepoint needs no unwinding of its own.
        Err(e) => return Err(e.into()),
    }
    if crate::search_text::is_searched(op.entity, field) {
        let row = crate::search_text::Rows::Uuid(&op.entity_uuid);
        match op.entity {
            Entity::Object => crate::search_text::refresh_objects(&mut *tx, row).await?,
            _ => crate::search_text::refresh_activities(&mut *tx, row).await?,
        };
    }

    record::stamp_field_clock(
        &mut *tx,
        op.entity,
        &op.entity_uuid,
        field,
        &op.edited_at,
        &op.device_id,
    )
    .await?;

    Ok(Outcome::Accepted)
}
