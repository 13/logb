use crate::auth::AuthUser;
use crate::db;
use crate::domain::tags;
use crate::error::AppError;
use crate::state::App;
use crate::sync::{record, Entity};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use std::collections::HashMap;
// Named here so the moved handlers' `super::` paths still reach their `api` siblings.
use super::{insights, normalize_client_uuid, reminders, CLIENT_UUID_TAKEN};

mod query;
mod write;

pub use query::{load_owned_object, load_owned_object_on, stats, ListQuery, ObjectScope};
use query::{list, read, with_stats};
use write::{create, delete, update};

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects", get(list).post(create))
        .route("/objects/{id}", get(read).patch(update).delete(delete))
}

#[derive(Serialize, sqlx::FromRow, Clone, Debug)]
pub struct ObjectRow {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub type_: String,
    pub counter_unit: Option<String>,
    pub fuel_unit: Option<String>,
    pub description: String,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i64>,
    pub archived_at: Option<String>,
    pub cover_attachment_id: Option<i64>,
    /// The object this one sits inside, or `None` for a root. Validated identically on both
    /// doors -- see `record::parent_is_valid`.
    pub parent_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    /// The row's sync identity, so a client can name it in an op without a bootstrap first.
    pub client_uuid: Option<String>,
    /// The stored JSON text, kept as text so sync and export can pass it through untouched;
    /// responses carry it as the array it holds.
    #[serde(serialize_with = "tags::serialize_json_text")]
    pub tags: String,
    /// Cents per `fuel_unit` times 1000 (0.30 EUR/kWh -> 30000), the same scale as
    /// `cost_per_counter_milli`. Nullable, and only meaningful alongside a `fuel_unit` -- see
    /// `ObjectInput::validate`. `>= 0` when present.
    pub energy_price_milli: Option<i64>,
    pub weight_unit: String,
    pub fuel_capacity_milli: Option<i64>,
    /// Neutral successor to `fuel_unit`. Both are returned during the compatibility window.
    pub resource_unit: Option<String>,
    pub resource_kind: Option<String>,
    pub measurement_mode: Option<String>,
    pub monthly_target_milli: Option<i64>,
    pub low_level_pct: Option<i64>,
    #[serde(rename = "private")]
    #[sqlx(rename = "private")]
    pub is_private: i64,
}

#[derive(Serialize, sqlx::FromRow, Clone, Debug)]
pub struct ObjectStats {
    pub total_cost_cents: i64,
    pub activity_count: i64,
    pub current_counter: Option<i64>,
    pub latest_weight_grams: Option<i64>,
    pub latest_weight_date: Option<String>,
    pub due_reminder_count: i64,
    /// The date of the newest entry with a counter value, up to `reminders::reading_horizon`.
    pub last_reading_date: Option<String>,
    /// The newest non-deleted activity dated today or earlier. A future-dated entry -- a planned
    /// expense -- is not recent activity.
    pub last_activity_date: Option<String>,
    /// Counter units per day over recent readings, scaled by 1000 -- the Info tab's figure, from
    /// `insights::usage_by_object`. Null until there is enough history.
    pub counter_per_day_milli: Option<i64>,
}

/// One link in an object's ancestor chain, as the client needs it to draw a breadcrumb:
/// a tuple would serialize as `[7, "House"]` and make the client index by position.
#[derive(Serialize, Clone, Debug)]
pub struct Ancestor {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize)]
pub struct ObjectOut {
    #[serde(flatten)]
    pub object: ObjectRow,
    pub stats: ObjectStats,
    /// file_id of the cover attachment, so the client can build a thumbnail URL directly
    pub cover_file_id: Option<i64>,
    /// Root first, nearest ancestor last, this object excluded. Filled in only by the
    /// single-object paths (`read`, `create`, `update`); the list leaves it empty rather than
    /// running one recursive query per row for a breadcrumb no list view draws.
    pub ancestors: Vec<Ancestor>,
}

#[derive(Deserialize)]
pub struct ObjectInput {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub counter_unit: Option<String>,
    #[serde(default)]
    pub fuel_unit: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub purchase_date: Option<String>,
    #[serde(default)]
    pub purchase_price_cents: Option<i64>,
    #[serde(default)]
    pub archived: Option<bool>,
    /// Three-state on PATCH: absent keeps the current cover, `null` clears it, an id sets it.
    /// A plain `Option` cannot tell "absent" from "null", which is why the cover could
    /// previously only ever be set, never removed.
    #[serde(default, deserialize_with = "double_option")]
    pub cover_attachment_id: Option<Option<i64>>,
    /// Three-state on PATCH, exactly as `cover_attachment_id` above: absent keeps the current
    /// parent, `null` makes the object a root again, an id moves it.
    #[serde(default, deserialize_with = "double_option")]
    pub parent_id: Option<Option<i64>>,
    /// Identity minted by the client before the server saw the row. A replay carrying the
    /// same value answers with the row the first attempt made. See `super::normalize_client_uuid`.
    #[serde(default)]
    pub client_uuid: Option<String>,
    /// Absent on create means no tags; absent on PATCH keeps the current ones. `validate`
    /// replaces a present list with its normalised form.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Three-state on PATCH, exactly as `cover_attachment_id` above: absent keeps the current
    /// price, `null` clears it, a number sets it. On POST, omitted and `null` both mean "no
    /// price". See `ENERGY_PRICE_NEEDS_FUEL_UNIT` and `validate` for the cross-field rule.
    #[serde(default, deserialize_with = "double_option")]
    pub energy_price_milli: Option<Option<i64>>,
    #[serde(default)]
    pub weight_unit: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub fuel_capacity_milli: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub resource_unit: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub resource_kind: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub measurement_mode: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub monthly_target_milli: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub low_level_pct: Option<Option<i64>>,
    #[serde(default)]
    pub private: Option<bool>,
}

/// The one sentence both doors answer a bad parent with -- `objects::update` as a 400 and
/// `sync::apply` as a rejection reason -- so a client cannot tell from the wording which door
/// it knocked on.
pub const PARENT_REJECTION: &str =
    "parent_id must be your own, undeleted, not itself, and not a descendant";

/// The 400 for a `type` that is neither built in nor one of the caller's own types. The same
/// sentence as before custom types existed, so no client has to learn a new one.
pub const TYPE_REJECTION: &str = "type is not one of the known object types";

/// The one sentence both doors answer a `counter_unit` change with, when it would leave a
/// stored trip's `counter_value` meaningless -- a trip is only ever allowed on an object whose
/// counter is `km` or `mi` (`ActivityInput::validate`), so an object that already has one may
/// never move its counter away from that pair. `km` and `mi` remain freely interchangeable:
/// only a change that leaves the pair (to `h`, or to no counter at all) is refused.
pub const COUNTER_UNIT_TRIP_REJECTION: &str =
    "this object has trips; its counter must stay km or mi";

/// The 400 (and sync rejection) for a stored or incoming `energy_price_milli` with no
/// `fuel_unit` to price -- whether the price is what just changed, or `fuel_unit` was cleared
/// out from under an already-stored price. See `ObjectInput::validate` and the block in
/// `update` below that checks a price kept from `existing` against a `fuel_unit` the same PATCH
/// clears; `sync::apply` answers the same two directions with the same sentence.
pub const ENERGY_PRICE_NEEDS_FUEL_UNIT: &str = "energy_price_milli needs a fuel unit";
pub const FUEL_UNIT_HISTORY_REJECTION: &str =
    "this object has fuel entries; its fuel unit cannot change";

/// Refuses a type the caller may not use. On the write transaction's connection, so a type
/// deleted concurrently cannot slip in between this check and the write (see `types::delete`).
async fn check_type(
    conn: &mut sqlx::AnyConnection,
    user_id: i64,
    type_key: &str,
) -> Result<(), AppError> {
    if crate::object_type::is_valid_for_user(&mut *conn, user_id, type_key).await? {
        Ok(())
    } else {
        Err(AppError::BadRequest(TYPE_REJECTION.into()))
    }
}

/// Deserializes a present field -- including an explicit `null` -- as `Some(..)`, leaving
/// `None` to mean "the client did not send this field at all".
///
/// `pub(crate)`: `ActivityInput`'s trip fields (`start_counter`, `from_place`, `to_place`,
/// `duration_minutes`, `battery_used_pct` in `api::activities`) are three-state on PATCH for
/// the same reason `cover_attachment_id` and `parent_id` are here, and reuse this rather than
/// carry a second copy of the same deserializer.
pub(crate) fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(d).map(Some)
}

pub fn validate_date(s: &str) -> Result<(), AppError> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| AppError::BadRequest(format!("invalid date '{s}', expected YYYY-MM-DD")))
}

impl ObjectInput {
    pub(crate) fn validate(&mut self) -> Result<(), AppError> {
        if self
            .weight_unit
            .as_deref()
            .is_some_and(|u| !matches!(u, "kg" | "lb"))
        {
            return Err(AppError::BadRequest("weight_unit must be kg or lb".into()));
        }
        if self.fuel_capacity_milli.flatten().is_some_and(|v| v <= 0) {
            return Err(AppError::BadRequest(
                "fuel_capacity_milli must be > 0".into(),
            ));
        }
        if self.monthly_target_milli.flatten().is_some_and(|v| v <= 0) {
            return Err(AppError::BadRequest(
                "monthly_target_milli must be > 0".into(),
            ));
        }
        if self
            .low_level_pct
            .flatten()
            .is_some_and(|v| !(0..=100).contains(&v))
        {
            return Err(AppError::BadRequest(
                "low_level_pct must be between 0 and 100".into(),
            ));
        }
        if self
            .resource_unit
            .as_ref()
            .and_then(|v| v.as_deref())
            .is_some_and(|u| !matches!(u, "l" | "gal" | "kwh" | "m3"))
        {
            return Err(AppError::BadRequest(
                "resource_unit must be l, gal, kwh, m3 or null".into(),
            ));
        }
        if self
            .resource_kind
            .as_ref()
            .and_then(|v| v.as_deref())
            .is_some_and(|k| {
                !matches!(k, "electricity" | "heating_fuel" | "vehicle_fuel" | "water")
            })
        {
            return Err(AppError::BadRequest("resource_kind is invalid".into()));
        }
        if self
            .measurement_mode
            .as_ref()
            .and_then(|v| v.as_deref())
            .is_some_and(|m| !matches!(m, "usage" | "meter"))
        {
            return Err(AppError::BadRequest(
                "measurement_mode must be usage, meter or null".into(),
            ));
        }
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            return Err(AppError::BadRequest("name is required".into()));
        }
        // Only normalised here: whether the type exists depends on the caller's own types, which
        // takes the database, so `create` and `update` check it inside their write transaction.
        self.type_ = self.type_.trim().to_lowercase();
        if let Some(u) = &self.counter_unit {
            if !matches!(u.as_str(), "km" | "mi" | "h") {
                return Err(AppError::BadRequest(
                    "counter_unit must be km, mi, h or null".into(),
                ));
            }
        }
        if let Some(u) = &self.fuel_unit {
            if !matches!(u.as_str(), "l" | "gal" | "kwh") {
                return Err(AppError::BadRequest(
                    "fuel_unit must be l, gal, kwh or null".into(),
                ));
            }
        }
        if let Some(d) = &self.purchase_date {
            validate_date(d)?;
        }
        if matches!(self.purchase_price_cents, Some(p) if p < 0) {
            return Err(AppError::BadRequest(
                "purchase_price_cents must be >= 0".into(),
            ));
        }
        if let Some(t) = &self.tags {
            self.tags = Some(tags::normalize(t).map_err(AppError::BadRequest)?);
        }
        // Self-contained half of the price/fuel_unit rule: an explicit price (present and
        // non-null) is checked here against this same body's `fuel_unit`, which is always fully
        // resent (never three-state) -- see `ObjectInput`'s own doc comment on `fuel_unit`. A
        // PATCH that OMITS the price keeps the stored one instead (three-state, like
        // `cover_attachment_id`), so that half of the rule cannot be decided from the body alone
        // and is checked in `create`/`update` once the stored value is known.
        if let Some(Some(p)) = self.energy_price_milli {
            if p < 0 {
                return Err(AppError::BadRequest(
                    "energy_price_milli must be >= 0".into(),
                ));
            }
            if self.fuel_unit.is_none() && self.resource_unit.as_ref().is_none_or(|v| v.is_none()) {
                return Err(AppError::BadRequest(ENERGY_PRICE_NEEDS_FUEL_UNIT.into()));
            }
        }
        Ok(())
    }
}
