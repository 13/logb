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

use crate::api::objects::PARENT_REJECTION;
use crate::domain::custom_type::{self, TypeInput, CUSTOM_PREFIX};
use crate::domain::tags;
use crate::error::AppError;
use crate::sync::record;
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
/// `attachments.kind`); everything else has nothing standing between a client and the row without
/// this. `objects.type` needs the database (a user's own types), so `apply_op` checks it, and an
/// object type's fields need the rest of the stored type -- see `type_field`.
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
            matches!((entity, field), (Entity::Reminder, "repeat_months" | "repeat_counter" | "every_n"));
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
        (Entity::Object, "name" | "type")
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

/// Whether `field` is a `tags` column, the one field whose pushed text is rewritten rather than
/// only checked.
fn is_tags(entity: Entity, field: &str) -> bool {
    matches!((entity, field), (Entity::Object | Entity::Activity, "tags"))
}

/// The stored spelling of a pushed `tags` value: JSON text holding an array of strings, run
/// through the same `domain::tags::normalize` REST uses, so the two doors store the same tags.
/// `Err` is the rejection reason.
pub fn canonical_tags(text: &str) -> Result<String, String> {
    let parsed: Vec<String> = serde_json::from_str(text)
        .map_err(|_| "tags must be JSON text holding an array of strings".to_string())?;
    tags::normalize(&parsed).map(|t| tags::to_json(&t))
}

/// A `set` op's value as it should be logged. The push handler writes `changes` BEFORE the op
/// is applied, so without this the log would carry the device's raw spelling of `tags` while
/// the row holds the normalised one, and every other device would pull a value the server
/// never stored. A value that does not normalise is returned untouched: `apply_op` rejects it,
/// and a rejected op's log row is removed.
pub fn canonical_value(
    entity: Entity,
    field: Option<&str>,
    value: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match (field, value) {
        (Some(field), Some(serde_json::Value::String(text))) if is_tags(entity, field) => {
            Some(serde_json::Value::String(canonical_tags(&text).unwrap_or(text)))
        }
        // An object type's name is stored trimmed and its categories normalised, so they are
        // logged that way too. Whatever fails here is rejected by `type_field`, and its log row
        // removed.
        (Some("name"), Some(serde_json::Value::String(text))) if entity == Entity::ObjectType => {
            Some(serde_json::Value::String(text.trim().to_string()))
        }
        (Some("categories"), Some(serde_json::Value::String(text))) if entity == Entity::ObjectType => {
            Some(serde_json::Value::String(canonical_categories(&text).unwrap_or(text)))
        }
        (_, value) => value,
    }
}

const CATEGORIES_SHAPE: &str = "categories must be JSON text holding an array of strings";

/// The stored spelling of a pushed object type `categories` value. `Err` is the rejection reason.
fn canonical_categories(text: &str) -> Result<String, String> {
    let parsed: Vec<String> = serde_json::from_str(text).map_err(|_| CATEGORIES_SHAPE.to_string())?;
    custom_type::normalize_categories(parsed).map(|c| serde_json::to_string(&c).unwrap_or_else(|_| "[]".into()))
}

/// The four fields a pushed object type `create` carries in `value`.
#[derive(serde::Deserialize)]
struct TypeValue {
    name: String,
    icon: String,
    categories: Vec<String>,
    #[serde(default)]
    counter_unit: Option<String>,
}

/// A pushed `create` of an object type. Unlike every other entity's create, which only announces
/// a row already made over REST, this inserts the row. Types are what a device makes offline
/// together with the objects that use them, and the objects refer to the type by this uuid. The
/// same rules as `POST /types` apply: `custom_type::normalize` and a unique name.
///
/// Applied in push order inside the push's one transaction, so a later op in the same batch
/// (`set object.type = custom:<uuid>`) already sees the row through `is_valid_for_user`.
async fn create_type(tx: &mut sqlx::AnyConnection, user_id: i64, op: &Op) -> Result<Outcome, AppError> {
    fn rejected(reason: impl Into<String>) -> Result<Outcome, AppError> {
        Ok(Outcome::Rejected { reason: reason.into() })
    }
    // Lower case only: the uuid becomes part of an object's `type`, which REST lower-cases, so
    // a mixed-case uuid would name a type no object could ever use.
    let shape_ok = crate::api::normalize_client_uuid(Some(op.entity_uuid.clone()))
        .is_ok_and(|u| u.as_deref() == Some(op.entity_uuid.as_str()));
    if !shape_ok || op.entity_uuid != op.entity_uuid.to_lowercase() {
        return rejected("an object type's entity_uuid must be 8-64 lower-case characters with no whitespace");
    }
    let existing: Option<(i64, Option<String>)> =
        sqlx::query_as("SELECT user_id, deleted_at FROM object_types WHERE client_uuid = $1")
            .bind(&op.entity_uuid)
            .fetch_optional(&mut *tx)
            .await?;
    match existing {
        // A replay of a create that already landed, for example through `POST /types`.
        Some((owner, None)) if owner == user_id => return Ok(Outcome::Accepted),
        // Another account's uuid, or a deleted type's: the uuid is spoken for, as on REST.
        Some(_) => return rejected("entity_uuid is already taken"),
        None => {}
    }
    let Some(value) = op.value.clone() else {
        return rejected("an object type create carries name, icon, categories and counter_unit in value");
    };
    let Ok(value) = serde_json::from_value::<TypeValue>(value) else {
        return rejected("an object type create carries name, icon, categories and counter_unit in value");
    };
    let input = TypeInput { name: value.name, icon: value.icon, categories: value.categories, counter_unit: value.counter_unit };
    let input = match custom_type::normalize(input) {
        Ok(input) => input,
        Err(reason) => return rejected(reason),
    };
    if crate::api::types::name_taken(tx, user_id, &input.name, None).await? {
        return rejected(crate::api::types::NAME_TAKEN);
    }
    let now = crate::db::now();
    sqlx::query(
        "INSERT INTO object_types (user_id, client_uuid, name, icon, categories, counter_unit, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(user_id).bind(&op.entity_uuid).bind(&input.name).bind(&input.icon)
        .bind(serde_json::to_string(&input.categories).unwrap_or_else(|_| "[]".into()))
        .bind(&input.counter_unit).bind(&now).bind(&now)
        .execute(&mut *tx).await?;
    // Every field was set by this create, at the op's own clock, for the reason
    // `record::record_create` gives: an unstamped field loses to nothing.
    for (field, _) in super::whitelist(Entity::ObjectType) {
        record::stamp_field_clock(tx, Entity::ObjectType, &op.entity_uuid, field, &op.edited_at, &op.device_id).await?;
    }
    Ok(Outcome::Accepted)
}

/// Checks a pushed `set` on an object type against the whole type: the stored row with this one
/// field replaced goes through `custom_type::normalize`, as a `PATCH /types` body would, and a
/// new name must not collide with the user's other types. Answers the value to store (trimmed,
/// normalised) or the rejection reason.
async fn type_field(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    uuid: &str,
    field: &str,
    bound: Binding,
) -> Result<Result<Binding, String>, AppError> {
    let (id, name, icon, categories, counter_unit): (i64, String, String, String, Option<String>) = sqlx::query_as(
        "SELECT id, name, icon, categories, counter_unit FROM object_types WHERE client_uuid = $1")
        .bind(uuid)
        .fetch_one(&mut *tx)
        .await?;
    let mut input = TypeInput { name, icon, categories: serde_json::from_str(&categories).unwrap_or_default(), counter_unit };
    let text = match bound {
        Binding::Text(text) => Some(text),
        Binding::Null => None,
        // `binding` has already refused anything but text or null for these TEXT fields.
        Binding::Integer(_) => return Ok(Err(format!("{field} must be a string"))),
    };
    match (field, text) {
        ("counter_unit", unit) => input.counter_unit = unit,
        (_, None) => return Ok(Err(format!("{field} is required"))),
        ("name", Some(text)) => input.name = text,
        ("icon", Some(text)) => input.icon = text,
        ("categories", Some(text)) => match serde_json::from_str::<Vec<String>>(&text) {
            Ok(parsed) => input.categories = parsed,
            Err(_) => return Ok(Err(CATEGORIES_SHAPE.into())),
        },
        _ => return Ok(Err(format!("{field} is not settable"))),
    }
    let input = match custom_type::normalize(input) {
        Ok(input) => input,
        Err(reason) => return Ok(Err(reason)),
    };
    if field == "name" && crate::api::types::name_taken(tx, user_id, &input.name, Some(id)).await? {
        return Ok(Err(crate::api::types::NAME_TAKEN.into()));
    }
    Ok(Ok(match field {
        "name" => Binding::Text(input.name),
        "icon" => Binding::Text(input.icon),
        "categories" => Binding::Text(serde_json::to_string(&input.categories).unwrap_or_else(|_| "[]".into())),
        _ => input.counter_unit.map_or(Binding::Null, Binding::Text),
    }))
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
    code.parse::<i32>().is_ok_and(|code| code & 0xff == SQLITE_CONSTRAINT)
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
        return Ok(Outcome::Rejected { reason: "entity_uuid and device_id are required".into() });
    }

    // The one create that inserts, and so the one op whose row need not exist yet.
    if op.entity == Entity::ObjectType && op.op == OpKind::Create {
        return create_type(tx, user_id, op).await;
    }

    // Does this uuid exist, and does it belong to the caller?
    let owner: Option<i64> = match op.entity {
        Entity::Object => sqlx::query_scalar(
            "SELECT user_id FROM objects WHERE client_uuid = $1")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::Activity => sqlx::query_scalar(
            "SELECT o.user_id FROM activities a JOIN objects o ON o.id = a.object_id \
             WHERE a.client_uuid = $1")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::Reminder => sqlx::query_scalar(
            "SELECT o.user_id FROM reminders r JOIN objects o ON o.id = r.object_id \
             WHERE r.client_uuid = $1")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::Attachment => sqlx::query_scalar(
            "SELECT o.user_id FROM attachments t JOIN objects o ON o.id = t.object_id \
             WHERE t.client_uuid = $1")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::File => sqlx::query_scalar(
            "SELECT user_id FROM files WHERE client_uuid = $1")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::ObjectType => sqlx::query_scalar(
            "SELECT user_id FROM object_types WHERE client_uuid = $1")
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

            // The same refusal as `DELETE /types/{id}`: an object whose type vanished would have no
            // icon and no categories. Counted under the push's write lock, so no object can take
            // the type between this count and the tombstone.
            if op.entity == Entity::ObjectType {
                let in_use: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM objects WHERE user_id = $1 AND type = $2 AND deleted_at IS NULL")
                    .bind(user_id).bind(format!("{CUSTOM_PREFIX}{}", op.entity_uuid))
                    .fetch_one(&mut *tx).await?;
                if in_use > 0 {
                    return Ok(Outcome::Rejected { reason: format!("in use by {in_use} object(s)") });
                }
            }

            let now = crate::db::now();
            // Only `objects`, `activities` and `object_types` carry `updated_at`;
            // `reminders`, `attachments` and `files` do not. The REST delete handlers stamp it
            // alongside `deleted_at` wherever the column exists, so this has to too, or a row
            // tombstoned over sync keeps whatever `updated_at` it had before the delete.
            let has_updated_at = matches!(op.entity, Entity::Object | Entity::Activity | Entity::ObjectType);
            let sql = if has_updated_at {
                format!(
                    "UPDATE {} SET deleted_at = $1, updated_at = $2 \
                     WHERE client_uuid = $3 AND deleted_at IS NULL",
                    op.entity.table()
                )
            } else {
                format!(
                    "UPDATE {} SET deleted_at = $1 WHERE client_uuid = $2 AND deleted_at IS NULL",
                    op.entity.table()
                )
            };
            let query = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(&now);
            let query = if has_updated_at { query.bind(&now) } else { query };
            query.bind(&op.entity_uuid).execute(&mut *tx).await?;

            let cascaded = match op.entity {
                Entity::Object => record::cascade_object(&mut *tx, &op.entity_uuid, &now).await?,
                Entity::Activity => record::cascade_activity(&mut *tx, user_id, &op.entity_uuid, &now, &op.edited_at).await?,
                // An attachment has no children to tombstone, but it is not a leaf reference-wise:
                // it can be an object's cover, and `cover_attachment_id` is a plain INTEGER with
                // no FK to enforce that by itself -- see `record::clear_cover_of`.
                Entity::Attachment => {
                    record::clear_cover_of(&mut *tx, user_id, &op.entity_uuid, &op.edited_at).await?;
                    Vec::new()
                }
                // A reminder has no children of its own, and nothing else keeps a stray
                // reference to it that a delete would need to clean up.
                Entity::Reminder => Vec::new(),
                // Nothing points at a type by id; objects that use it were refused above.
                Entity::ObjectType => Vec::new(),
                Entity::File => unreachable!("a file delete is refused above, before reaching this match"),
            };
            record::log_cascade(&mut *tx, user_id, &op.edited_at, &op.device_id, &cascaded).await?;
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
                other => other,
            };

            // A type key needs the database: a built-in key, or one of the caller's own live types
            // -- including one a create earlier in this same push inserted, on this transaction.
            if op.entity == Entity::Object && field == "type" {
                if let Binding::Text(key) = &bound {
                    if !crate::object_type::is_valid_for_user(&mut *tx, user_id, key).await? {
                        return Ok(Outcome::Rejected { reason: crate::api::objects::TYPE_REJECTION.into() });
                    }
                }
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
                             WHERE a.client_uuid = $1 AND o.deleted_at IS NULL")
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
                         WHERE a.id = $1 AND o.client_uuid = $2 AND a.deleted_at IS NULL")
                        .bind(referenced).bind(&op.entity_uuid)
                        .fetch_optional(&mut *tx).await?,
                    // A parent is not "a row on the same object" but a row on the same
                    // ACCOUNT that must also not be inside this object's own subtree, so it
                    // goes through `record::parent_is_valid` -- the single copy of that rule
                    // the REST door uses too.
                    (Entity::Object, "parent_id") => {
                        let object_id = record::id_of(&mut *tx, Entity::Object, &op.entity_uuid).await?;
                        if record::parent_is_valid(&mut *tx, user_id, Some(object_id), *referenced).await? {
                            Some(*referenced)
                        } else {
                            None
                        }
                    }
                    (Entity::Reminder, "done_activity_id") => sqlx::query_scalar(
                        "SELECT act.id FROM activities act \
                         JOIN reminders r ON r.object_id = act.object_id \
                         WHERE act.id = $1 AND r.client_uuid = $2 AND act.deleted_at IS NULL")
                        .bind(referenced).bind(&op.entity_uuid)
                        .fetch_optional(&mut *tx).await?,
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
            // `tests/concurrency.rs`'s `the_later_edit_wins_regardless_of_arrival_order` fails
            // on PostgreSQL without it (and stays green on SQLite, whose own `BEGIN IMMEDIATE`
            // already serialises writers). Do not add a lock here -- the one this depends on is
            // already held for the whole push.
            let stored: Option<(String, String)> = sqlx::query_as(
                "SELECT edited_at, device_id FROM field_clock \
                 WHERE entity = $1 AND entity_uuid = $2 AND field = $3")
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
            // `tests/sync.rs` pins that, asserting the writes before and after really landed.
            //
            // The name is a literal, and one `set` op is never nested inside another, so a
            // single name cannot collide with itself.
            sqlx::query("SAVEPOINT logb_set_op").execute(&mut *tx).await?;
            match query.bind(&op.entity_uuid).execute(&mut *tx).await {
                Ok(_) => {
                    sqlx::query("RELEASE SAVEPOINT logb_set_op").execute(&mut *tx).await?;
                },
                Err(e) if is_constraint_violation(&e) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT logb_set_op").execute(&mut *tx).await?;
                    return Ok(Outcome::Rejected {
                        reason: format!("{field} violates a database constraint"),
                    });
                },
                // Not a constraint failure: a genuine fault, and the whole batch is rolled
                // back with it, so the savepoint needs no unwinding of its own.
                Err(e) => return Err(e.into()),
            }

            record::stamp_field_clock(
                &mut *tx, op.entity, &op.entity_uuid, field, &op.edited_at, &op.device_id,
            )
            .await?;

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
