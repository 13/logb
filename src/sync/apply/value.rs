//! Turning a client's JSON value into something safe to bind to a column, and the canonical
//! spellings a stored value has to take.

use crate::domain::custom_type;
use crate::domain::tags;
use crate::sync::{Entity, FieldType};

/// A client value that has been checked against its column's type and is ready to bind.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Binding {
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
pub(super) fn binding(
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
pub(super) fn validate_value(entity: Entity, field: &str, bound: &Binding) -> Result<(), String> {
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
pub(super) fn is_tags(entity: Entity, field: &str) -> bool {
    matches!((entity, field), (Entity::Object | Entity::Activity, "tags"))
}

/// Whether `field` is a trip place, the other kind of field whose pushed text is rewritten --
/// trimmed, blank becomes absent -- rather than only checked.
pub(super) fn is_place(entity: Entity, field: &str) -> bool {
    matches!(
        (entity, field),
        (Entity::Activity, "from_place" | "to_place")
    )
}

/// A trip place's stored spelling: trimmed, `None` when what remains is blank -- the same rule
/// `ActivityInput`'s `trim_place` applies on the REST door, so a value pushed over sync and one
/// written over REST end up identical. Unlike `canonical_tags`/`canonical_categories`, trimming
/// cannot fail; the 80-character limit is `validate_value`'s to enforce, once, on this result.
pub(super) fn canonical_place(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// An activity row's trip-relevant columns, read fresh from the database for the cross-field
/// checks below -- a named struct rather than a wide tuple, which clippy's `type_complexity`
/// refuses to let through, and which would be unreadable at every call site besides.
#[derive(sqlx::FromRow)]
pub(super) struct TripRow {
    pub(super) category: String,
    pub(super) counter_value: Option<i64>,
    pub(super) start_counter: Option<i64>,
    pub(super) from_place: Option<String>,
    pub(super) to_place: Option<String>,
    pub(super) duration_minutes: Option<i64>,
    pub(super) battery_used_pct: Option<i64>,
    /// The object's `counter_unit`, joined in because only a `km`/`mi` object may hold a trip.
    pub(super) counter_unit: Option<String>,
}

impl TripRow {
    /// Whether any of the five trip-only fields is stored -- the same test
    /// `ActivityInput::validate` names `has_trip_fields`, over the row instead of a REST body.
    pub(super) fn has_trip_fields(&self) -> bool {
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

pub(super) const CATEGORIES_SHAPE: &str = "categories must be JSON text holding an array of strings";

/// The stored spelling of a pushed object type `categories` value. `Err` is the rejection reason.
pub(super) fn canonical_categories(text: &str) -> Result<String, String> {
    let parsed: Vec<String> =
        serde_json::from_str(text).map_err(|_| CATEGORIES_SHAPE.to_string())?;
    custom_type::normalize_categories(parsed)
        .map(|c| serde_json::to_string(&c).unwrap_or_else(|_| "[]".into()))
        .map_err(String::from)
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

