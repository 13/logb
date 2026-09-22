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
        let Binding::Integer(n) = bound else {
            return Ok(());
        };
        // `repeat_months`/`repeat_counter` must be strictly positive -- see
        // `ReminderInput::validate` -- which is a stricter bound than "non-negative" and
        // therefore satisfies it too.
        let must_be_positive = matches!(
            (entity, field),
            (
                Entity::Reminder,
                "repeat_months" | "repeat_counter" | "every_n"
            )
        );
        let non_negative = matches!(
            (entity, field),
            (
                Entity::Object,
                "purchase_price_cents" | "energy_price_milli"
            ) | (
                Entity::Activity,
                "cost_cents" | "counter_value" | "quantity_milli" | "start_counter"
            ) | (Entity::Reminder, "due_counter")
        );
        if must_be_positive && *n <= 0 {
            return Err(format!("{field} must be > 0"));
        }
        if entity == Entity::Object && field == "fuel_capacity_milli" && *n <= 0 {
            return Err("fuel_capacity_milli must be > 0".into());
        }
        if entity == Entity::Object && field == "monthly_target_milli" && *n <= 0 {
            return Err("monthly_target_milli must be > 0".into());
        }
        if entity == Entity::Object && field == "low_level_pct" && !(0..=100).contains(n) {
            return Err("low_level_pct must be between 0 and 100".into());
        }
        if non_negative && *n < 0 {
            return Err(format!("{field} must be >= 0"));
        }
        // The same bounds `ActivityInput::validate` enforces on the REST door -- see the
        // Global Constraints in the trip log spec.
        if entity == Entity::Activity && field == "battery_used_pct" && !(0..=100).contains(n) {
            return Err("battery_used_pct must be between 0 and 100".into());
        }
        if entity == Entity::Activity && field == "fuel_level_pct" && !(0..=100).contains(n) {
            return Err("fuel_level_pct must be between 0 and 100".into());
        }
        if entity == Entity::Activity && field == "duration_minutes" && !(1..=10080).contains(n) {
            return Err("duration_minutes must be between 1 and 10080".into());
        }
        // `charged_full` is a flag, not a free integer -- the same 0/1 bound
        // `ActivityInput::validate` enforces on the REST door.
        if entity == Entity::Activity && field == "weight_grams" && !(1..=1_000_000_000).contains(n)
        {
            return Err("weight_grams must be between 1 and 1000000000".into());
        }
        if entity == Entity::Activity && field == "charged_full" && !(0..=1).contains(n) {
            return Err("charged_full must be 0 or 1".into());
        }
        if matches!(
            (entity, field),
            (Entity::Object, "private") | (Entity::Activity, "estimated" | "meter_reset")
        ) && !(0..=1).contains(n)
        {
            return Err(format!("{field} must be 0 or 1"));
        }
        if entity == Entity::Activity && field == "meter_reading_milli" && *n < 0 {
            return Err("meter_reading_milli must be >= 0".into());
        }
        return Ok(());
    };
    if entity == Entity::Reminder && field == "schedule"
        && crate::domain::reminder::CalendarSchedule::parse(text).is_none()
    {
        return Err("invalid calendar schedule".into());
    }

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
        // By the time this runs the text has already been through `canonical_place` (the push
        // handler runs `canonical_value` before `apply_op` ever sees the op, and the `Set` arm
        // below runs it again on `bound` directly, exactly as it does for tags) -- so this is
        // the TRIMMED length, matching `ActivityInput::validate`'s `chars().count()` check.
        (Entity::Activity, "from_place" | "to_place") if text.chars().count() > 80 => {
            return Err("from_place and to_place must be at most 80 characters".into());
        }
        // `counter_unit`/`fuel_unit` each carry a SQLite/PostgreSQL CHECK constraint that
        // happens to reject anything outside their own whitelist too (the module doc comment
        // above calls this out), and `apply_op`'s savepoint already turns that into a clean
        // `Rejected` rather than a 500 -- but only with a generic "violates a database
        // constraint" reason, not the specific one `ObjectInput::validate` gives the REST door
        // for the exact same mistake. Explicit arms here make the two doors agree word for word,
        // and stop depending on a constraint that is schema, not policy, to enforce it at all.
        (Entity::Object, "weight_unit") if !matches!(text.as_str(), "kg" | "lb") => {
            return Err("weight_unit must be kg or lb".into())
        }
        (Entity::Object, "counter_unit") if !matches!(text.as_str(), "km" | "mi" | "h") => {
            return Err("counter_unit must be km, mi, h or null".into());
        }
        (Entity::Object, "fuel_unit") if !matches!(text.as_str(), "l" | "gal" | "kwh") => {
            return Err("fuel_unit must be l, gal, kwh or null".into());
        }
        (Entity::Object, "resource_unit")
            if !matches!(text.as_str(), "l" | "gal" | "kwh" | "m3") =>
        {
            return Err("resource_unit must be l, gal, kwh, m3 or null".into())
        }
        (Entity::Object, "resource_kind")
            if !matches!(
                text.as_str(),
                "electricity" | "heating_fuel" | "vehicle_fuel" | "water"
            ) =>
        {
            return Err("resource_kind is invalid".into())
        }
        (Entity::Object, "measurement_mode") if !matches!(text.as_str(), "usage" | "meter") => {
            return Err("measurement_mode must be usage, meter or null".into())
        }
        (Entity::Activity, "period_start" | "period_end") => {
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

/// Whether `field` is a trip place, the other kind of field whose pushed text is rewritten --
/// trimmed, blank becomes absent -- rather than only checked.
fn is_place(entity: Entity, field: &str) -> bool {
    matches!(
        (entity, field),
        (Entity::Activity, "from_place" | "to_place")
    )
}

/// A trip place's stored spelling: trimmed, `None` when what remains is blank -- the same rule
/// `ActivityInput`'s `trim_place` applies on the REST door, so a value pushed over sync and one
/// written over REST end up identical. Unlike `canonical_tags`/`canonical_categories`, trimming
/// cannot fail; the 80-character limit is `validate_value`'s to enforce, once, on this result.
fn canonical_place(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// An activity row's trip-relevant columns, read fresh from the database for the cross-field
/// checks below -- a named struct rather than a wide tuple, which clippy's `type_complexity`
/// refuses to let through, and which would be unreadable at every call site besides.
#[derive(sqlx::FromRow)]
struct TripRow {
    category: String,
    counter_value: Option<i64>,
    start_counter: Option<i64>,
    from_place: Option<String>,
    to_place: Option<String>,
    duration_minutes: Option<i64>,
    battery_used_pct: Option<i64>,
    /// The object's `counter_unit`, joined in because only a `km`/`mi` object may hold a trip.
    counter_unit: Option<String>,
}

impl TripRow {
    /// Whether any of the five trip-only fields is stored -- the same test
    /// `ActivityInput::validate` names `has_trip_fields`, over the row instead of a REST body.
    fn has_trip_fields(&self) -> bool {
        self.start_counter.is_some()
            || self.from_place.is_some()
            || self.to_place.is_some()
            || self.duration_minutes.is_some()
            || self.battery_used_pct.is_some()
    }
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
        (Some(field), Some(serde_json::Value::String(text))) if is_tags(entity, field) => Some(
            serde_json::Value::String(canonical_tags(&text).unwrap_or(text)),
        ),
        (Some(field), Some(serde_json::Value::String(text))) if is_place(entity, field) => {
            Some(canonical_place(&text).map_or(serde_json::Value::Null, serde_json::Value::String))
        }
        // An object type's name is stored trimmed and its categories normalised, so they are
        // logged that way too. Whatever fails here is rejected by `type_field`, and its log row
        // removed.
        (Some("name"), Some(serde_json::Value::String(text))) if entity == Entity::ObjectType => {
            Some(serde_json::Value::String(text.trim().to_string()))
        }
        (Some("categories"), Some(serde_json::Value::String(text)))
            if entity == Entity::ObjectType =>
        {
            Some(serde_json::Value::String(
                canonical_categories(&text).unwrap_or(text),
            ))
        }
        (_, value) => value,
    }
}

const CATEGORIES_SHAPE: &str = "categories must be JSON text holding an array of strings";

/// The stored spelling of a pushed object type `categories` value. `Err` is the rejection reason.
fn canonical_categories(text: &str) -> Result<String, String> {
    let parsed: Vec<String> =
        serde_json::from_str(text).map_err(|_| CATEGORIES_SHAPE.to_string())?;
    custom_type::normalize_categories(parsed)
        .map(|c| serde_json::to_string(&c).unwrap_or_else(|_| "[]".into()))
        .map_err(String::from)
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
async fn create_type(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    op: &Op,
) -> Result<Outcome, AppError> {
    fn rejected(reason: impl Into<String>) -> Result<Outcome, AppError> {
        Ok(Outcome::Rejected {
            reason: reason.into(),
        })
    }
    // Lower case only: the uuid becomes part of an object's `type`, which REST lower-cases, so
    // a mixed-case uuid would name a type no object could ever use.
    let shape_ok = crate::api::normalize_client_uuid(Some(op.entity_uuid.clone()))
        .is_ok_and(|u| u.as_deref() == Some(op.entity_uuid.as_str()));
    if !shape_ok || op.entity_uuid != op.entity_uuid.to_lowercase() {
        return rejected(
            "an object type's entity_uuid must be 8-64 lower-case characters with no whitespace",
        );
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
        return rejected(
            "an object type create carries name, icon, categories and counter_unit in value",
        );
    };
    let Ok(value) = serde_json::from_value::<TypeValue>(value) else {
        return rejected(
            "an object type create carries name, icon, categories and counter_unit in value",
        );
    };
    let input = TypeInput {
        name: value.name,
        icon: value.icon,
        categories: value.categories,
        counter_unit: value.counter_unit,
    };
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
        record::stamp_field_clock(
            tx,
            Entity::ObjectType,
            &op.entity_uuid,
            field,
            &op.edited_at,
            &op.device_id,
        )
        .await?;
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
    let mut input = TypeInput {
        name,
        icon,
        categories: serde_json::from_str(&categories).unwrap_or_default(),
        counter_unit,
    };
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
        Err(reason) => return Ok(Err(reason.message)),
    };
    if field == "name" && crate::api::types::name_taken(tx, user_id, &input.name, Some(id)).await? {
        return Ok(Err(crate::api::types::NAME_TAKEN.into()));
    }
    Ok(Ok(match field {
        "name" => Binding::Text(input.name),
        "icon" => Binding::Text(input.icon),
        "categories" => {
            Binding::Text(serde_json::to_string(&input.categories).unwrap_or_else(|_| "[]".into()))
        }
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
    chrono::DateTime::parse_from_rfc3339(raw).ok().map(|t| {
        t.with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    })
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

/// The reminder fields whose value only makes sense against the rest of the row: an interval is
/// legal on its own and illegal beside a calendar schedule, and `apply_op` sees one field at a
/// time. Anything listed here is re-checked by `revalidate_reminder`.
const REVALIDATED_REMINDER_FIELDS: [&str; 7] = [
    "schedule",
    "every_n",
    "every_unit",
    "repeat_months",
    "repeat_counter",
    "due_counter",
    "due_date",
];

/// Answers the rejection reason for a field write that the whole-row rules refuse, or `None` when
/// the row would still be valid with that field replaced.
///
/// The stored row is turned into JSON, the one field is overwritten, and the result is read back
/// as a `ReminderInput` so `validate` -- the same function the REST handler calls -- decides. The
/// round-trip looks gratuitous next to a hand-written check, and that is the point: a rule added
/// to `validate` applies to sync writes without anyone remembering to copy it here.
async fn revalidate_reminder(
    tx: &mut sqlx::AnyConnection,
    client_uuid: &str,
    field: &str,
    value: Option<&serde_json::Value>,
) -> Result<Option<String>, AppError> {
    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        String,
    );
    let (
        title,
        notes,
        due_date,
        due_counter,
        repeat_months,
        repeat_counter,
        kind,
        every_n,
        every_unit,
        schedule,
        uuid,
        counter_unit,
        object_type,
    ): Row = sqlx::query_as(
        "SELECT r.title, r.notes, r.due_date, r.due_counter, r.repeat_months, r.repeat_counter, \
                r.kind, r.every_n, r.every_unit, r.schedule, r.client_uuid, o.counter_unit, o.type \
         FROM reminders r JOIN objects o ON o.id = r.object_id WHERE r.client_uuid = $1",
    )
    .bind(client_uuid)
    .fetch_one(&mut *tx)
    .await?;
    let mut row = serde_json::json!({
        "title": title,
        "notes": notes,
        "due_date": due_date,
        "due_counter": due_counter,
        "repeat_months": repeat_months,
        "repeat_counter": repeat_counter,
        "kind": kind,
        "every_n": every_n,
        "every_unit": every_unit,
        "schedule": schedule,
        "client_uuid": uuid,
    });
    row[field] = value.cloned().unwrap_or(serde_json::Value::Null);
    let mut input: crate::api::reminders::ReminderInput =
        serde_json::from_value(row).map_err(|e| AppError::Internal(e.to_string()))?;
    // A body object logs weight in grams and has no counter column, but its reading reminders are
    // valid all the same -- `validate` asks for the unit, so it gets the one weight is kept in.
    let unit = if object_type == "body" { Some("g") } else { counter_unit.as_deref() };
    Ok(input.validate(unit).err().map(|e| e.to_string()))
}

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
                    return Ok(Outcome::Rejected {
                        reason: format!("in use by {in_use} object(s)"),
                    });
                }
            }

            let now = crate::db::now();
            // Only `objects`, `activities` and `object_types` carry `updated_at`;
            // `reminders`, `attachments` and `files` do not. The REST delete handlers stamp it
            // alongside `deleted_at` wherever the column exists, so this has to too, or a row
            // tombstoned over sync keeps whatever `updated_at` it had before the delete.
            let has_updated_at = matches!(
                op.entity,
                Entity::Object | Entity::Activity | Entity::ObjectType
            );
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
            let query = if has_updated_at {
                query.bind(&now)
            } else {
                query
            };
            query.bind(&op.entity_uuid).execute(&mut *tx).await?;

            let cascaded = match op.entity {
                Entity::Object => record::cascade_object(&mut *tx, &op.entity_uuid, &now).await?,
                Entity::Activity => {
                    record::cascade_activity(
                        &mut *tx,
                        user_id,
                        &op.entity_uuid,
                        &now,
                        &op.edited_at,
                    )
                    .await?
                }
                // An attachment has no children to tombstone, but it is not a leaf reference-wise:
                // it can be an object's cover, and `cover_attachment_id` is a plain INTEGER with
                // no FK to enforce that by itself -- see `record::clear_cover_of`.
                Entity::Attachment => {
                    record::clear_cover_of(&mut *tx, user_id, &op.entity_uuid, &op.edited_at)
                        .await?;
                    Vec::new()
                }
                // A reminder has no children of its own, and nothing else keeps a stray
                // reference to it that a delete would need to clean up.
                Entity::Reminder => Vec::new(),
                // Nothing points at a type by id; objects that use it were refused above.
                Entity::ObjectType => Vec::new(),
                Entity::File => {
                    unreachable!("a file delete is refused above, before reaching this match")
                }
            };
            record::log_cascade(&mut *tx, user_id, &op.edited_at, &op.device_id, &cascaded).await?;
            Ok(Outcome::Accepted)
        }

        OpKind::Set => {
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

            // The trip cross-field rules: something no single field's shape or range can say on
            // its own, so they are checked against the row's OTHER current values, the way
            // `ActivityInput::validate` checks them against the rest of a REST PATCH's body. A
            // REST PATCH resends every field together and is validated as one combination; a
            // synced `set` touches exactly one field, checked against what is stored RIGHT NOW
            // -- so a client meaning to change several of these together, especially `category`
            // to or from `trip`, has to either order its ops so each one lands in a state the
            // next one's check can pass, or send the whole change as one REST PATCH instead,
            // which is what a category change to or from `trip` is meant to go through (see
            // `docs/openapi.json`'s `/sync/push` description).
            //
            // This also has to sit ahead of the last-write-wins comparison below, not after:
            // an op that is invalid against the row as it stands is Rejected outright, on
            // purpose, never Superseded. Superseded is for a stale but otherwise valid value
            // losing a race it would have won moments earlier; invalid data is never stored
            // regardless of timing, so an old, invalid op must not reach the one comparison
            // that only ever decides who wins a race between two values that were each fine on
            // their own.
            if op.entity == Entity::Activity
                && matches!(
                    field,
                    "category"
                        | "start_counter"
                        | "from_place"
                        | "to_place"
                        | "duration_minutes"
                        | "battery_used_pct"
                        | "counter_value"
                )
            {
                let stored: TripRow = sqlx::query_as(
                    "SELECT a.category, a.counter_value, a.start_counter, a.from_place, a.to_place, \
                     a.duration_minutes, a.battery_used_pct, o.counter_unit \
                     FROM activities a JOIN objects o ON o.id = a.object_id WHERE a.client_uuid = $1",
                )
                .bind(&op.entity_uuid)
                .fetch_one(&mut *tx)
                .await?;

                const ONLY_A_TRIP: &str =
                    "only a trip has start_counter, places, duration or battery";
                const NEEDS_BOTH: &str = "a trip needs start_counter and counter_value";
                const START_RANGE: &str = "start_counter must be between 0 and counter_value";
                const NEEDS_KM_MI: &str = "a trip needs an object that counts km or mi";

                if field == "category" {
                    // A `null` category cannot reach here as `Binding::Text`; `binding` already
                    // shaped it as `Binding::Null`, and the column's own NOT NULL constraint is
                    // what refuses that, exactly as it does for any other required TEXT field.
                    if let Binding::Text(new_category) = &bound {
                        if new_category == "trip" {
                            if !matches!(stored.counter_unit.as_deref(), Some("km") | Some("mi")) {
                                return Ok(Outcome::Rejected {
                                    reason: NEEDS_KM_MI.into(),
                                });
                            }
                            match (stored.start_counter, stored.counter_value) {
                                (Some(s), Some(e)) if (0..=e).contains(&s) => {}
                                (Some(_), Some(_)) => {
                                    return Ok(Outcome::Rejected {
                                        reason: START_RANGE.into(),
                                    })
                                }
                                _ => {
                                    return Ok(Outcome::Rejected {
                                        reason: NEEDS_BOTH.into(),
                                    })
                                }
                            }
                        } else if new_category != "session" && stored.has_trip_fields() {
                            return Ok(Outcome::Rejected {
                                reason: ONLY_A_TRIP.into(),
                            });
                        }
                    }
                } else if field == "counter_value" {
                    // Not trip-only -- every category may carry one -- so only checked against
                    // `start_counter` when the row IS a trip, and a trip may never lose it: an
                    // end with no start left is exactly the row `ActivityInput::validate`
                    // refuses to create in the first place.
                    if stored.category == "trip" {
                        match &bound {
                            Binding::Integer(n)
                                if stored.start_counter.is_some_and(|s| (0..=*n).contains(&s)) => {}
                            Binding::Integer(_) => {
                                return Ok(Outcome::Rejected {
                                    reason: START_RANGE.into(),
                                })
                            }
                            _ => {
                                return Ok(Outcome::Rejected {
                                    reason: NEEDS_BOTH.into(),
                                })
                            }
                        }
                    }
                } else if stored.category != "trip" && stored.category != "session" {
                    // The four trip-only fields, plus `start_counter`: refused outright on a
                    // non-trip row, unless they are being cleared -- clearing stays legal
                    // regardless of category, exactly as on the REST door.
                    if !matches!(&bound, Binding::Null) {
                        return Ok(Outcome::Rejected {
                            reason: ONLY_A_TRIP.into(),
                        });
                    }
                } else if stored.category == "session" {
                    if matches!(field, "start_counter" | "to_place" | "battery_used_pct")
                        && !matches!(&bound, Binding::Null)
                    {
                        return Ok(Outcome::Rejected {
                            reason: "a session only has a place and duration".into(),
                        });
                    }
                } else if field == "start_counter" {
                    // On a trip row specifically: a trip may never lose its start either,
                    // mirroring `counter_value` above.
                    match &bound {
                        Binding::Integer(n)
                            if stored.counter_value.is_some_and(|e| (0..=e).contains(n)) => {}
                        Binding::Integer(_) => {
                            return Ok(Outcome::Rejected {
                                reason: START_RANGE.into(),
                            })
                        }
                        _ => {
                            return Ok(Outcome::Rejected {
                                reason: NEEDS_BOTH.into(),
                            })
                        }
                    }
                }
                // The three other trip-only fields need no further check here on a trip row:
                // clearing or resetting `from_place`/`to_place`/`duration_minutes`/
                // `battery_used_pct` never makes the row invalid the way losing `start_counter`
                // or `counter_value` would.
            }

            // The charge cross-field rule, independent of the trip checks above: `charged_full`
            // may only ever sit at 1 on a `fuel` row, in either direction -- setting the flag
            // directly, or moving the row's own category away from `fuel` while the flag is
            // still stored. A separate query rather than folding into `TripRow`/its `matches!`
            // list above: `charged_full` is not one of the trip-only fields that block's
            // catch-all (`stored.category != "trip"`) exists to guard, and it needs none of
            // that block's other state.
            if op.entity == Entity::Activity
                && matches!(
                    field,
                    "category" | "weight_grams" | "counter_value" | "quantity_milli" | "cost_cents"
                )
            {
                let (category, weight, counter, quantity, cost): (String, Option<i64>, Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
                    "SELECT category, weight_grams, counter_value, quantity_milli, cost_cents FROM activities WHERE client_uuid = $1")
                    .bind(&op.entity_uuid).fetch_one(&mut *tx).await?;
                let value = op.value.as_ref().unwrap_or(&serde_json::Value::Null);
                let category = if field == "category" {
                    value.as_str().unwrap_or("")
                } else {
                    &category
                };
                let weight = if field == "weight_grams" {
                    value.as_i64()
                } else {
                    weight
                };
                let counter = if field == "counter_value" {
                    value.as_i64()
                } else {
                    counter
                };
                let quantity = if field == "quantity_milli" {
                    value.as_i64()
                } else {
                    quantity
                };
                let cost = if field == "cost_cents" {
                    value.as_i64()
                } else {
                    cost
                };
                if (category == "weight"
                    && (weight.is_none()
                        || counter.is_some()
                        || quantity.is_some()
                        || cost.is_some()))
                    || (category != "weight" && weight.is_some())
                {
                    return Ok(Outcome::Rejected { reason: "weight entries require weight_grams and cannot carry counters, fuel or costs".into() });
                }
            }
            if op.entity == Entity::Activity && matches!(field, "category" | "charged_full") {
                let (stored_category, stored_charged_full): (String, i64) = sqlx::query_as(
                    "SELECT category, charged_full FROM activities WHERE client_uuid = $1",
                )
                .bind(&op.entity_uuid)
                .fetch_one(&mut *tx)
                .await?;
                if field == "charged_full" {
                    if matches!(&bound, Binding::Integer(1))
                        && !matches!(stored_category.as_str(), "fuel" | "usage")
                    {
                        return Ok(Outcome::Rejected {
                            reason: crate::api::activities::CHARGE_FULL_ONLY.into(),
                        });
                    }
                } else if let Binding::Text(new_category) = &bound {
                    if !matches!(new_category.as_str(), "fuel" | "usage")
                        && stored_charged_full == 1
                    {
                        return Ok(Outcome::Rejected {
                            reason: crate::api::activities::CHARGE_FULL_ONLY.into(),
                        });
                    }
                }
            }
            if op.entity == Entity::Activity
                && matches!(field, "category" | "fuel_level_pct" | "quantity_milli")
            {
                let (stored_category, stored_level, stored_quantity, fuel_unit, counter_unit): (String, Option<i64>, Option<i64>, Option<String>, Option<String>) = sqlx::query_as(
                    "SELECT a.category, a.fuel_level_pct, a.quantity_milli, o.fuel_unit, o.counter_unit FROM activities a JOIN objects o ON o.id = a.object_id WHERE a.client_uuid = $1")
                    .bind(&op.entity_uuid).fetch_one(&mut *tx).await?;
                let new_category = if field == "category" {
                    match &bound {
                        Binding::Text(v) => v.as_str(),
                        _ => stored_category.as_str(),
                    }
                } else {
                    stored_category.as_str()
                };
                let new_level = if field == "fuel_level_pct" {
                    match &bound {
                        Binding::Integer(v) => Some(*v),
                        Binding::Null => None,
                        _ => stored_level,
                    }
                } else {
                    stored_level
                };
                let new_quantity = if field == "quantity_milli" {
                    match &bound {
                        Binding::Integer(v) => Some(*v),
                        Binding::Null => None,
                        _ => stored_quantity,
                    }
                } else {
                    stored_quantity
                };
                if new_level.is_some() && !matches!(new_category, "fuel" | "usage") {
                    return Ok(Outcome::Rejected {
                        reason: "only a fuel entry has fuel_level_pct".into(),
                    });
                }
                if new_level.is_some() && !matches!(fuel_unit.as_deref(), Some("l") | Some("gal")) {
                    return Ok(Outcome::Rejected {
                        reason: "fuel_level_pct needs a liquid fuel unit".into(),
                    });
                }
                if new_quantity.is_some() && !matches!(new_category, "fuel" | "usage") {
                    return Ok(Outcome::Rejected {
                        reason: "only a fuel entry has quantity_milli".into(),
                    });
                }
                if new_quantity.is_some() && fuel_unit.is_none() && counter_unit.is_none() {
                    return Ok(Outcome::Rejected {
                        reason: "quantity_milli needs an object with a fuel or counter unit".into(),
                    });
                }
            }

            // The energy price cross-field rule: a price may exist only alongside a fuel unit,
            // checked against the row as it stands right now -- mirrors
            // `ObjectInput::validate`'s combined check on the REST door, for the same reason the
            // `counter_unit`/trip guard below reads the stored row instead of the rest of the
            // PATCH body: a synced `set` touches one field at a time, so the other half of the
            // pair has to come from storage.
            if op.entity == Entity::Object && matches!(field, "energy_price_milli" | "fuel_unit") {
                let (stored_fuel_unit, stored_price): (Option<String>, Option<i64>) =
                    sqlx::query_as(
                        "SELECT fuel_unit, energy_price_milli FROM objects WHERE client_uuid = $1",
                    )
                    .bind(&op.entity_uuid)
                    .fetch_one(&mut *tx)
                    .await?;
                let needs_fuel_unit = if field == "energy_price_milli" {
                    !matches!(&bound, Binding::Null) && stored_fuel_unit.is_none()
                } else {
                    matches!(&bound, Binding::Null) && stored_price.is_some()
                };
                if needs_fuel_unit {
                    return Ok(Outcome::Rejected {
                        reason: crate::api::objects::ENERGY_PRICE_NEEDS_FUEL_UNIT.into(),
                    });
                }
            }
            if op.entity == Entity::Object && matches!(field, "fuel_capacity_milli" | "fuel_unit") {
                let (stored_fuel_unit, stored_capacity): (Option<String>, Option<i64>) =
                    sqlx::query_as(
                        "SELECT fuel_unit, fuel_capacity_milli FROM objects WHERE client_uuid = $1",
                    )
                    .bind(&op.entity_uuid)
                    .fetch_one(&mut *tx)
                    .await?;
                let unit = if field == "fuel_unit" {
                    match &bound {
                        Binding::Text(v) => Some(v.as_str()),
                        Binding::Null => None,
                        _ => stored_fuel_unit.as_deref(),
                    }
                } else {
                    stored_fuel_unit.as_deref()
                };
                let capacity = if field == "fuel_capacity_milli" {
                    !matches!(&bound, Binding::Null)
                } else {
                    stored_capacity.is_some()
                };
                if capacity && !matches!(unit, Some("l") | Some("gal")) {
                    return Ok(Outcome::Rejected {
                        reason: "fuel_capacity_milli needs a liquid fuel unit".into(),
                    });
                }
            }
            if op.entity == Entity::Object && field == "fuel_unit" {
                let current: Option<String> =
                    sqlx::query_scalar("SELECT fuel_unit FROM objects WHERE client_uuid = $1")
                        .bind(&op.entity_uuid)
                        .fetch_one(&mut *tx)
                        .await?;
                let next = match &bound {
                    Binding::Text(v) => Some(v.as_str()),
                    Binding::Null => None,
                    _ => current.as_deref(),
                };
                if next != current.as_deref() {
                    let has_history: Option<(i64,)> = sqlx::query_as(
                        "SELECT a.id FROM activities a JOIN objects o ON o.id = a.object_id WHERE o.client_uuid = $1 AND a.deleted_at IS NULL AND (a.quantity_milli IS NOT NULL OR a.fuel_level_pct IS NOT NULL) LIMIT 1")
                        .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?;
                    if has_history.is_some() {
                        return Ok(Outcome::Rejected {
                            reason: crate::api::objects::FUEL_UNIT_HISTORY_REJECTION.into(),
                        });
                    }
                }
            }

            // A type key needs the database: a built-in key, or one of the caller's own live types
            // -- including one a create earlier in this same push inserted, on this transaction.
            if op.entity == Entity::Object && field == "type" {
                if let Binding::Text(key) = &bound {
                    if !crate::object_type::is_valid_for_user(&mut *tx, user_id, key).await? {
                        return Ok(Outcome::Rejected {
                            reason: crate::api::objects::TYPE_REJECTION.into(),
                        });
                    }
                }
            }

            // A trip needs its object to keep counting km or mi -- see the doc comment on
            // `api::objects::COUNTER_UNIT_TRIP_REJECTION`, which both doors answer with. `km`
            // and `mi` are each other's only legal replacement, so this only has to ask about
            // the ones that leave that pair: `h`, and clearing the counter entirely.
            if op.entity == Entity::Object
                && field == "counter_unit"
                && !matches!(&bound, Binding::Text(u) if matches!(u.as_str(), "km" | "mi"))
            {
                let has_trip: Option<(i64,)> = sqlx::query_as(
                    "SELECT a.id FROM activities a JOIN objects o ON o.id = a.object_id \
                     WHERE o.client_uuid = $1 AND a.category = 'trip' AND a.deleted_at IS NULL LIMIT 1",
                )
                .bind(&op.entity_uuid)
                .fetch_optional(&mut *tx)
                .await?;
                if has_trip.is_some() {
                    return Ok(Outcome::Rejected {
                        reason: crate::api::objects::COUNTER_UNIT_TRIP_REJECTION.into(),
                    });
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
            // `tests/concurrency.rs`'s `the_later_edit_wins_regardless_of_arrival_order` fails
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
            // `tests/sync.rs` pins that, asserting the writes before and after really landed.
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
