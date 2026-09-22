//! Whole-row re-validation for the reminder fields a single-field write can invalidate.

use crate::error::AppError;

/// The reminder fields whose value only makes sense against the rest of the row: an interval is
/// legal on its own and illegal beside a calendar schedule, and `apply_op` sees one field at a
/// time. Anything listed here is re-checked by `revalidate_reminder`.
pub(super) const REVALIDATED_REMINDER_FIELDS: [&str; 7] = [
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
pub(super) async fn revalidate_reminder(
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

