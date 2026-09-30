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
use crate::sync::{Entity, Op, OpKind, Outcome};

mod activity;
mod delete;
mod object;
mod reminder;
mod set;
mod types;
mod value;

pub use value::{canonical_edited_at, canonical_tags, canonical_value};
use delete::apply_delete;
use set::apply_set;
use types::create_type;

/// Whether a database error is a constraint violation, i.e. the statement was refused for what
/// it tried to store rather than because anything is wrong with the database.
///
/// SQLite reports every constraint failure with `SQLITE_CONSTRAINT` (19) as the primary result
/// code in the low byte of the extended code it hands back: 1299 NOT NULL, 275 CHECK, 787
/// FOREIGN KEY, 2067 UNIQUE, and the rest. Testing the low byte therefore catches every subtype,
/// including the ones `DatabaseError::kind()` folds into `ErrorKind::Other`, while still letting
/// an I/O error, a locked database or a schema fault through as the genuine 500 it is.
///
/// PostgreSQL says the same thing in SQLSTATE: class `23` is "integrity constraint violation",
/// covering 23502 NOT NULL, 23503 FOREIGN KEY, 23505 UNIQUE and 23514 CHECK. Without this half,
/// every constraint failure on PostgreSQL was a 500 that threw away the whole batch -- the
/// exact failure the `rejected` bucket exists to avoid.
///
/// The two are told apart by length rather than by asking which backend is connected, because
/// the error is all this has: a SQLSTATE is always five characters, while SQLite's extended
/// codes are at most four digits (the primary code in the low byte, a small subtype above it).
/// Parsing first would misread `23502` as a number and test the wrong byte of it.
fn is_constraint_violation(err: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db) = err else {
        return false;
    };
    let Some(code) = db.code() else {
        return false;
    };
    if code.len() == PG_SQLSTATE_LEN {
        return code.starts_with(PG_INTEGRITY_CONSTRAINT_CLASS);
    }
    code.parse::<i32>()
        .is_ok_and(|code| code & 0xff == SQLITE_CONSTRAINT)
}

/// SQLite's primary result code for a constraint violation.
const SQLITE_CONSTRAINT: i32 = 19;

/// Every PostgreSQL SQLSTATE is exactly this long, which is what separates one from a SQLite
/// extended result code.
const PG_SQLSTATE_LEN: usize = 5;

/// The SQLSTATE class PostgreSQL reports every integrity constraint violation under.
const PG_INTEGRITY_CONSTRAINT_CLASS: &str = "23";


/// Applies one op inside the caller's transaction and returns how it landed.
///
/// Ownership is resolved by joining back to `objects.user_id` rather than trusting anything in
/// the op, so a uuid belonging to another account cannot be written through.
pub async fn apply_op(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    op: &Op,
) -> Result<Outcome, AppError> {
    if op.entity_uuid.is_empty() || op.device_id.is_empty() {
        return Ok(Outcome::Rejected {
            reason: "entity_uuid and device_id are required".into(),
        });
    }

    // The one create that inserts, and so the one op whose row need not exist yet.
    if op.entity == Entity::ObjectType && op.op == OpKind::Create {
        return create_type(tx, user_id, op).await;
    }

    // Does this uuid exist, and does it belong to the caller?
    let owner: Option<i64> = match op.entity {
        Entity::Object => {
            sqlx::query_scalar("SELECT user_id FROM objects WHERE client_uuid = $1")
                .bind(&op.entity_uuid)
                .fetch_optional(&mut *tx)
                .await?
        }
        Entity::Activity => {
            sqlx::query_scalar(
                "SELECT o.user_id FROM activities a JOIN objects o ON o.id = a.object_id \
             WHERE a.client_uuid = $1",
            )
            .bind(&op.entity_uuid)
            .fetch_optional(&mut *tx)
            .await?
        }
        Entity::Reminder => {
            sqlx::query_scalar(
                "SELECT o.user_id FROM reminders r JOIN objects o ON o.id = r.object_id \
             WHERE r.client_uuid = $1",
            )
            .bind(&op.entity_uuid)
            .fetch_optional(&mut *tx)
            .await?
        }
        Entity::Attachment => {
            sqlx::query_scalar(
                "SELECT o.user_id FROM attachments t JOIN objects o ON o.id = t.object_id \
             WHERE t.client_uuid = $1",
            )
            .bind(&op.entity_uuid)
            .fetch_optional(&mut *tx)
            .await?
        }
        Entity::File => {
            sqlx::query_scalar("SELECT user_id FROM files WHERE client_uuid = $1")
                .bind(&op.entity_uuid)
                .fetch_optional(&mut *tx)
                .await?
        }
        Entity::ObjectType => {
            sqlx::query_scalar("SELECT user_id FROM object_types WHERE client_uuid = $1")
                .bind(&op.entity_uuid)
                .fetch_optional(&mut *tx)
                .await?
        }
    };
    match owner {
        None => {
            return Ok(Outcome::Rejected {
                reason: "unknown entity_uuid".into(),
            })
        }
        Some(owner) if owner != user_id => {
            return Ok(Outcome::Rejected {
                reason: "unknown entity_uuid".into(),
            })
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
        OpKind::Delete => apply_delete(tx, user_id, op).await,

        OpKind::Set => apply_set(tx, user_id, op).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::FieldType;
    use value::{binding, Binding};

    #[test]
    fn a_newer_edit_wins() {
        assert!(wins(
            "2026-01-02T00:00:00Z",
            "phone",
            "2026-01-01T00:00:00Z",
            "desktop"
        ));
    }

    #[test]
    fn an_older_edit_loses() {
        assert!(!wins(
            "2026-01-01T00:00:00Z",
            "phone",
            "2026-01-02T00:00:00Z",
            "desktop"
        ));
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
        assert_eq!(
            binding("counter_value", FieldType::Integer, Some(&json!(7))),
            Ok(Binding::Integer(7))
        );
        assert_eq!(
            binding("counter_value", FieldType::Integer, None),
            Ok(Binding::Null)
        );
        assert_eq!(
            binding("counter_value", FieldType::Integer, Some(&json!(null))),
            Ok(Binding::Null)
        );
        for bad in [json!("abc"), json!(true), json!(1.5), json!([1]), json!({})] {
            assert_eq!(
                binding("counter_value", FieldType::Integer, Some(&bad)),
                Err("counter_value must be an integer".into()),
                "{bad} is not an integer"
            );
        }

        // And the other direction: `true` in a TEXT column would have been stored as '1'.
        assert_eq!(
            binding("name", FieldType::Text, Some(&json!("Golf"))),
            Ok(Binding::Text("Golf".into()))
        );
        assert_eq!(
            binding("name", FieldType::Text, Some(&json!(null))),
            Ok(Binding::Null)
        );
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
        assert!(
            !wins(t, "phone", t, "phone"),
            "identical edit is not newer than itself"
        );
    }
}
