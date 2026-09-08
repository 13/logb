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

        OpKind::Delete => {
            let sql = format!(
                "UPDATE {} SET deleted_at = ? WHERE client_uuid = ? AND deleted_at IS NULL",
                op.entity.table()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql)).bind(crate::db::now()).bind(&op.entity_uuid)
                .execute(&mut *tx).await?;
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
