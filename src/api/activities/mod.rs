//! An activity: the row itself, what a client may send, and how one is read back.

use crate::api::attachments::{self, AttachmentOut};
use crate::api::objects::{validate_date, ObjectRow};
use crate::domain::tags;
use crate::error::AppError;
use crate::state::App;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};

mod query;
mod write;

pub(crate) use query::fold_title;
pub use query::ListQuery;
pub(crate) use query::recent_titles;
use query::{last_done, list};
pub(crate) use write::{create, delete};
use write::{read, update};


// Kept in sync with the CHECK on activities.category (migrations 0009, 0011 and 0015 on
// SQLite, 0001, 0002 and 0006 on PostgreSQL) and with frontend/src/lib/types.ts's CATEGORIES:
// four health categories alongside the original seven; `reading`, an entry that is nothing but
// a counter value; and `trip`, an entry whose end is `counter_value` and whose start is the
// `start_counter` column (see the doc comment on `ActivityRow::start_counter`).
pub const CATEGORIES: [&str; 16] = [
    "maintenance",
    "repair",
    "purchase",
    "inspection",
    "modification",
    "fuel",
    "usage",
    "other",
    "symptom",
    "treatment",
    "appointment",
    "medication",
    "reading",
    "trip",
    "weight",
    "session",
];

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects/{id}/activities", get(list).post(create))
        .route("/objects/{id}/recent-titles", get(recent_titles))
        .route("/objects/{id}/last-done", get(last_done))
        .route("/activities/{id}", get(read).patch(update).delete(delete))
}

#[derive(Serialize, sqlx::FromRow, Clone, Debug)]
pub struct ActivityRow {
    pub id: i64,
    pub object_id: i64,
    pub date: String,
    pub category: String,
    pub title: String,
    pub notes: String,
    pub counter_value: Option<i64>,
    pub cost_cents: Option<i64>,
    pub quantity_milli: Option<i64>,
    pub client_op_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// The row's sync identity, so a client can name it in an op without a bootstrap first.
    pub client_uuid: Option<String>,
    /// Stored JSON text, answered as the array it holds -- see `ObjectRow::tags`.
    #[serde(serialize_with = "tags::serialize_json_text")]
    pub tags: String,
    /// A trip's start; its end is the existing `counter_value` above, so the object's current
    /// counter, counter reminders and usage per month all keep reading it unchanged. `None` on
    /// every other category -- see `ActivityInput::validate`. Distance (`counter_value -
    /// start_counter`) is never stored, only computed where it is shown.
    pub start_counter: Option<i64>,
    /// A trip's start and end place, trimmed, blank stored as `None`. Used only by `trip`.
    pub from_place: Option<String>,
    pub to_place: Option<String>,
    /// A trip's duration in minutes, 1..=10080 (one week). Used only by `trip`.
    pub duration_minutes: Option<i64>,
    /// A trip's battery used, as a percentage, 0..=100. Used only by `trip`.
    pub battery_used_pct: Option<i64>,
    /// Whether this charge (or fill) topped the battery/tank up: 1 only on a `fuel` entry, and
    /// 0 on every other row (never `NULL` -- see the migration). A flag on the entry rather than
    /// a table, so it travels through sync, export and offline edits like every other field.
    pub charged_full: i64,
    pub weight_grams: Option<i64>,
    pub fuel_level_pct: Option<i64>,
    pub meter_reading_milli: Option<i64>,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub estimated: i64,
    pub meter_reset: i64,
}

#[derive(Deserialize)]
pub struct ActivityInput {
    pub date: String,
    pub category: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub counter_value: Option<i64>,
    #[serde(default)]
    pub cost_cents: Option<i64>,
    #[serde(default)]
    pub quantity_milli: Option<i64>,
    /// Client-generated id for this creation attempt. Present only from the offline outbox.
    #[serde(default)]
    pub client_op_id: Option<String>,
    /// When an edit was made, for an edit that reaches the server later than that -- one queued
    /// offline. Each field it changes is then written only if nothing newer has changed that
    /// field in the meantime (see `update`). Absent for an ordinary edit, which is made now.
    #[serde(default)]
    pub edited_at: Option<String>,
    /// Identity minted by the client before the server saw the row. A replay carrying the
    /// same value answers with the row the first attempt made. See `crate::api::normalize_client_uuid`.
    #[serde(default)]
    pub client_uuid: Option<String>,
    /// Absent on create means no tags; absent on PATCH keeps the current ones. `validate`
    /// replaces a present list with its normalised form.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Three-state on PATCH, exactly as `ObjectInput::cover_attachment_id`: absent keeps the
    /// stored value (`update` merges it in before calling `validate`, below), `null` clears it,
    /// a value sets it. Absent on create is simply "no value" -- `validate`'s `.flatten()`
    /// reads an absent outer `None` the same way as an explicit `null`.
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub start_counter: Option<Option<i64>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub from_place: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub to_place: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub duration_minutes: Option<Option<i64>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub battery_used_pct: Option<Option<i64>>,
    /// Absent on create means "not full" (0); absent on PATCH keeps the current value -- the
    /// same rule `tags` follows, not the three-state `double_option` the trip fields use above,
    /// because a charge can never be cleared to "no value", only ever set to 0 or 1.
    #[serde(default)]
    pub charged_full: Option<i64>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub weight_grams: Option<Option<i64>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub fuel_level_pct: Option<Option<i64>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub meter_reading_milli: Option<Option<i64>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub period_start: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub period_end: Option<Option<String>>,
    pub estimated: Option<i64>,
    pub meter_reset: Option<i64>,
}

/// A trip's place, trimmed; blank becomes `None`. `Err` if what remains is over 80 characters,
/// counted with `chars().count()` rather than bytes so "Bäckerei Müller" is judged by its own
/// 15 letters, not by how many bytes UTF-8 spends on the umlaut.
///
/// `pub(crate)`: `api::export::import` binds the same trimmed spelling into the row it inserts,
/// rather than the archive's raw text, so an import stores the identical value a REST create of
/// the same body would -- see the call there for why `validate_import` alone, which discards
/// the trimmed value it computes, is not enough.
pub(crate) fn trim_place(place: Option<String>) -> Result<Option<String>, AppError> {
    let Some(p) = place else { return Ok(None) };
    let trimmed = p.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > 80 {
        return Err(AppError::BadRequest(
            "from_place and to_place must be at most 80 characters".into(),
        ));
    }
    Ok(Some(trimmed.to_string()))
}

/// The one sentence both doors answer a misplaced `charged_full` with -- `ActivityInput::validate`
/// as a 400 and `sync::apply` as a rejection reason -- whichever direction broke it: setting the
/// flag on a non-`fuel` row, or moving a flagged row's category away from `fuel` while it is
/// still stored.
pub const CHARGE_FULL_ONLY: &str = "only a charge can be marked full";

impl ActivityInput {
    pub(crate) fn validate(&mut self, object: &ObjectRow) -> Result<(), AppError> {
        validate_date(&self.date)?;
        if !CATEGORIES.contains(&self.category.as_str()) {
            return Err(AppError::BadRequest(format!(
                "category must be one of {}",
                CATEGORIES.join(", ")
            )));
        }
        self.title = self.title.trim().to_string();
        // A trip's title defaults to "Trip" ($t('cat.trip')) in the UI when left blank, and a
        // charge's likewise defaults to "Charged"/"Geladen" or the petrol wording
        // ($t(energyLabelKey(fuel_unit))) -- see `activityTitle` on the frontend. Every other
        // category still requires one, exactly as before.
        if self.title.is_empty()
            && self.category != "trip"
            && self.category != "fuel"
            && self.category != "usage"
        {
            return Err(AppError::BadRequest("title is required".into()));
        }
        let weight = self.weight_grams.flatten();
        let fuel_level = self.fuel_level_pct.flatten();
        if fuel_level.is_some_and(|v| !(0..=100).contains(&v)) {
            return Err(AppError::BadRequest(
                "fuel_level_pct must be between 0 and 100".into(),
            ));
        }
        if fuel_level.is_some() && !matches!(self.category.as_str(), "fuel" | "usage") {
            return Err(AppError::BadRequest(
                "only a fuel entry has fuel_level_pct".into(),
            ));
        }
        if fuel_level.is_some()
            && !matches!(
                object
                    .resource_unit
                    .as_deref()
                    .or(object.fuel_unit.as_deref()),
                Some("l") | Some("gal")
            )
        {
            return Err(AppError::BadRequest(
                "fuel_level_pct needs a liquid fuel unit".into(),
            ));
        }
        if self.category == "weight" {
            if !weight.is_some_and(|w| (1..=1_000_000_000).contains(&w)) {
                return Err(AppError::BadRequest(
                    "weight_grams must be between 1 and 1000000000".into(),
                ));
            }
            if self.counter_value.is_some()
                || self.quantity_milli.is_some()
                || self.cost_cents.is_some()
            {
                return Err(AppError::BadRequest(
                    "weight entries cannot carry counters, fuel or costs".into(),
                ));
            }
        } else if weight.is_some() {
            return Err(AppError::BadRequest(
                "only a weight entry has weight_grams".into(),
            ));
        }
        if let Some(c) = self.counter_value {
            if object.counter_unit.is_none() {
                return Err(AppError::BadRequest("this object has no counter".into()));
            }
            if c < 0 {
                return Err(AppError::BadRequest("counter_value must be >= 0".into()));
            }
        }
        // A reading with no value records nothing at all.
        if self.category == "reading" && self.counter_value.is_none() {
            return Err(AppError::BadRequest("a reading needs counter_value".into()));
        }
        if matches!(self.cost_cents, Some(c) if c < 0) {
            return Err(AppError::BadRequest("cost_cents must be >= 0".into()));
        }
        if let Some(q) = self.quantity_milli {
            if q < 0 {
                return Err(AppError::BadRequest("quantity_milli must be >= 0".into()));
            }
            if !matches!(self.category.as_str(), "fuel" | "usage") {
                return Err(AppError::BadRequest(
                    "only a resource usage entry has quantity_milli".into(),
                ));
            }
            if object.resource_unit.is_none()
                && object.fuel_unit.is_none()
                && object.counter_unit.is_none()
            {
                return Err(AppError::BadRequest(
                    "quantity_milli needs an object with a resource or counter unit".into(),
                ));
            }
        }
        let meter = self.meter_reading_milli.flatten();
        if meter.is_some_and(|v| v < 0) {
            return Err(AppError::BadRequest(
                "meter_reading_milli must be >= 0".into(),
            ));
        }
        if meter.is_some()
            && (self.category != "usage" || object.resource_kind.as_deref() != Some("water"))
        {
            return Err(AppError::BadRequest(
                "meter readings need a water usage entry".into(),
            ));
        }
        if object.measurement_mode.as_deref() == Some("meter")
            && self.category == "usage"
            && meter.is_none()
        {
            return Err(AppError::BadRequest(
                "this water meter needs meter_reading_milli".into(),
            ));
        }
        if object.measurement_mode.as_deref() == Some("usage") && meter.is_some() {
            return Err(AppError::BadRequest(
                "this resource records period usage, not meter readings".into(),
            ));
        }
        let period_start = self.period_start.as_ref().and_then(|v| v.as_deref());
        let period_end = self.period_end.as_ref().and_then(|v| v.as_deref());
        if let Some(d) = period_start {
            validate_date(d)?;
        }
        if let Some(d) = period_end {
            validate_date(d)?;
        }
        if period_start.zip(period_end).is_some_and(|(a, b)| a > b) {
            return Err(AppError::BadRequest(
                "period_start must not be after period_end".into(),
            ));
        }
        if (period_start.is_some() || period_end.is_some()) && self.category != "usage" {
            return Err(AppError::BadRequest(
                "billing periods only belong to usage entries".into(),
            ));
        }
        for (name, value) in [
            ("estimated", self.estimated),
            ("meter_reset", self.meter_reset),
        ] {
            if value.is_some_and(|v| v != 0 && v != 1) {
                return Err(AppError::BadRequest(format!("{name} must be 0 or 1")));
            }
        }
        if self.meter_reset == Some(1) && meter.is_none() {
            return Err(AppError::BadRequest(
                "meter_reset needs a meter reading".into(),
            ));
        }
        if let Some(t) = &self.tags {
            self.tags = Some(tags::normalize(t).map_err(AppError::BadRequest)?);
        }
        if let Some(cf) = self.charged_full {
            if cf != 0 && cf != 1 {
                return Err(AppError::BadRequest("charged_full must be 0 or 1".into()));
            }
            if cf == 1 && !matches!(self.category.as_str(), "fuel" | "usage") {
                return Err(AppError::BadRequest(CHARGE_FULL_ONLY.into()));
            }
        }

        // The five trip fields, resolved to plain values: `.flatten()` reads an absent outer
        // `None` (create, or a PATCH `update` already merged the stored value into) the same
        // way as an explicit `null`. Read with `.take()` and written back below so every
        // caller -- `create`, `update` and `export::validate_import` alike -- can read the
        // trimmed, checked value back off `self` the same way afterwards, by `.flatten()`ing
        // again.
        let start_counter = self.start_counter.take().flatten();
        let duration_minutes = self.duration_minutes.take().flatten();
        let battery_used_pct = self.battery_used_pct.take().flatten();
        let from_place = trim_place(self.from_place.take().flatten())?;
        let to_place = trim_place(self.to_place.take().flatten())?;

        let has_trip_fields = start_counter.is_some()
            || from_place.is_some()
            || to_place.is_some()
            || duration_minutes.is_some()
            || battery_used_pct.is_some();
        if self.category != "trip" && self.category != "session" {
            if has_trip_fields {
                return Err(AppError::BadRequest(
                    "only a trip has start_counter, places, duration or battery".into(),
                ));
            }
        } else if self.category == "trip" {
            // Allowed only on an object with a distance counter, regardless of the object's
            // type's own category list -- see the module doc on `domain::custom_type` and the
            // spec's "Type categories" note: a `trip` is offered by counter unit, not by type.
            if !matches!(object.counter_unit.as_deref(), Some("km") | Some("mi")) {
                return Err(AppError::BadRequest(
                    "a trip needs an object that counts km or mi".into(),
                ));
            }
            let (Some(end), Some(start)) = (self.counter_value, start_counter) else {
                return Err(AppError::BadRequest(
                    "a trip needs start_counter and counter_value".into(),
                ));
            };
            if start < 0 || start > end {
                return Err(AppError::BadRequest(
                    "start_counter must be between 0 and counter_value".into(),
                ));
            }
        } else if start_counter.is_some() || to_place.is_some() || battery_used_pct.is_some() {
            return Err(AppError::BadRequest(
                "a session only has a place and duration".into(),
            ));
        }
        if let Some(b) = battery_used_pct {
            if !(0..=100).contains(&b) {
                return Err(AppError::BadRequest(
                    "battery_used_pct must be between 0 and 100".into(),
                ));
            }
        }
        if let Some(d) = duration_minutes {
            if !(1..=10080).contains(&d) {
                return Err(AppError::BadRequest(
                    "duration_minutes must be between 1 and 10080".into(),
                ));
            }
        }

        self.start_counter = Some(start_counter);
        self.duration_minutes = Some(duration_minutes);
        self.battery_used_pct = Some(battery_used_pct);
        self.from_place = Some(from_place);
        self.to_place = Some(to_place);
        Ok(())
    }
}

#[derive(Serialize)]
pub struct ActivityOut {
    #[serde(flatten)]
    pub activity: ActivityRow,
    pub attachments: Vec<AttachmentOut>,
}

pub async fn with_attachments(
    state: &App,
    rows: Vec<ActivityRow>,
) -> Result<Vec<ActivityOut>, AppError> {
    let object_id = match rows.first() {
        Some(r) => r.object_id,
        None => return Ok(vec![]),
    };
    let all = attachments::for_object(state, object_id).await?;
    Ok(rows
        .into_iter()
        .map(|activity| {
            let attachments = all
                .iter()
                .filter(|a| a.activity_id == Some(activity.id))
                .cloned()
                .collect();
            ActivityOut {
                activity,
                attachments,
            }
        })
        .collect())
}

async fn one_out(state: &App, row: ActivityRow) -> Result<ActivityOut, AppError> {
    Ok(with_attachments(state, vec![row]).await?.pop().unwrap())
}

pub async fn load_owned_activity(
    state: &App,
    user_id: i64,
    id: i64,
) -> Result<ActivityRow, AppError> {
    sqlx::query_as::<_, ActivityRow>(
        "SELECT a.id, a.object_id, a.date, a.category, a.title, a.notes, a.counter_value, a.cost_cents, \
         a.quantity_milli, a.client_op_id, a.created_at, a.updated_at, a.client_uuid, a.tags, \
         a.start_counter, a.from_place, a.to_place, a.duration_minutes, a.battery_used_pct, a.charged_full, a.weight_grams, a.fuel_level_pct, a.meter_reading_milli, a.period_start, a.period_end, a.estimated, a.meter_reset \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE a.id = $1 AND o.user_id = $2 AND a.deleted_at IS NULL AND o.deleted_at IS NULL",
    )
    .bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

