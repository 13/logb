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
use crate::sync::{is_syncable_field, Entity, Op, OpKind, Outcome};

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
            if !is_syncable_field(op.entity, field) {
                return Ok(Outcome::Rejected { reason: format!("{field} is not settable") });
            }

            // Two whitelisted fields are foreign keys, and a check on the field NAME says
            // nothing about the VALUE. Without this, `set object.cover_attachment_id` could
            // point a row at another account's attachment -- which the object read then hands
            // back as `cover_file_id`. The REST handlers already refuse a cross-object
            // reference; sync has to refuse it identically, or it is just a second, unguarded
            // door onto the same write.
            //
            // The check reads the VALUE, so a value that is not an id at all must not be able
            // to slip past it: SQLite columns are dynamically typed, and a string bound to
            // `cover_attachment_id` would be stored verbatim. Such a value cannot name another
            // account's row, but it corrupts an integer column, so for these two fields
            // anything that is neither an integer nor null is refused here rather than falling
            // through to the write. Null stays legal: it is how a client clears the reference.
            let is_foreign_key = matches!(
                (op.entity, field),
                (Entity::Object, "cover_attachment_id") | (Entity::Reminder, "done_activity_id")
            );
            let clearing = matches!(op.value, None | Some(serde_json::Value::Null));
            if let Some(referenced) = op.value.as_ref().and_then(serde_json::Value::as_i64) {
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
                    _ => Some(referenced),
                };
                if permitted.is_none() {
                    return Ok(Outcome::Rejected {
                        reason: format!("{field} must reference a row on the same object"),
                    });
                }
            } else if is_foreign_key && !clearing {
                return Ok(Outcome::Rejected {
                    reason: format!("{field} must be an integer id or null"),
                });
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
            let query = match op.value.as_ref() {
                None | Some(serde_json::Value::Null) => query.bind(None::<String>),
                Some(serde_json::Value::String(s)) => query.bind(s.clone()),
                Some(serde_json::Value::Number(n)) => query.bind(n.as_i64()),
                Some(serde_json::Value::Bool(b)) => query.bind(Some(i64::from(*b))),
                Some(other) => {
                    return Ok(Outcome::Rejected {
                        reason: format!("unsupported value type: {other}"),
                    })
                }
            };
            query.bind(&op.entity_uuid).execute(&mut *tx).await?;

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
    fn a_replay_of_the_same_op_does_not_win() {
        let t = "2026-01-01T00:00:00Z";
        assert!(!wins(t, "phone", t, "phone"), "identical edit is not newer than itself");
    }
}
