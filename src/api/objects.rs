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

/// The one statement both loaders below run, so a column added to `ObjectRow` cannot reach one
/// of them and not the other -- which would show up only as a decode error on whichever path
/// the tests happened not to cover.
const OWNED_OBJECT: &str =
    "SELECT id, user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
     purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags, \
     energy_price_milli, weight_unit, fuel_capacity_milli, resource_unit, resource_kind, measurement_mode, monthly_target_milli, low_level_pct, private \
     FROM objects WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL";

/// The object with `id` if it belongs to `user_id`; otherwise 404. Reads from the pool, for the
/// callers that only want to know the object exists and is theirs before they go on.
///
/// A caller that is about to *write* what it reads here wants `load_owned_object_on` instead:
/// see the comment in `update` for what a read taken before the write lock is worth.
pub async fn load_owned_object(state: &App, user_id: i64, id: i64) -> Result<ObjectRow, AppError> {
    sqlx::query_as::<_, ObjectRow>(OWNED_OBJECT)
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

/// `load_owned_object` on a connection the caller already holds -- in practice the one
/// `db::begin_write` has just taken the write lock on.
///
/// It has to be the transaction's own connection rather than a second one from the pool: a pool
/// connection acquired while `begin_write` holds SQLite's write lock does not fail, it hangs.
pub async fn load_owned_object_on(
    conn: &mut sqlx::AnyConnection,
    user_id: i64,
    id: i64,
) -> Result<ObjectRow, AppError> {
    sqlx::query_as::<_, ObjectRow>(OWNED_OBJECT)
        .bind(id)
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(AppError::NotFound)
}

/// One row of derived data per object: the stats block plus the cover's `file_id`.
#[derive(sqlx::FromRow)]
struct DerivedRow {
    object_id: i64,
    total_cost_cents: i64,
    activity_count: i64,
    current_counter: Option<i64>,
    latest_weight_grams: Option<i64>,
    latest_weight_date: Option<String>,
    due_reminder_count: i64,
    last_reading_date: Option<String>,
    last_activity_date: Option<String>,
    /// Filled in after the query, from `usage_by_object`, not read from a column.
    #[sqlx(default)]
    counter_per_day_milli: Option<i64>,
    cover_file_id: Option<i64>,
}

/// Everything `ObjectOut` needs beyond the `objects` row itself, for every object the user
/// owns, in one statement. `only` narrows it to a single object for the read/create/update
/// handlers, which keeps one copy of this SQL rather than a per-object and a per-list variant.
///
/// The list endpoint used to call `stats` and then a cover lookup once per object, so showing
/// N objects cost 2N + 1 queries.
async fn derived(
    state: &App,
    user_id: Option<i64>,
    only: Option<i64>,
) -> Result<HashMap<i64, DerivedRow>, AppError> {
    // INVARIANT: this due_reminder_count subquery is a second, hand-written encoding of
    // `domain::reminder::is_due` -- it exists only so N objects' counts can be computed in one
    // statement instead of loading every reminder and folding `is_due` over them in memory. The
    // two must keep agreeing row for row; `due_reminder_count_agrees_with_each_reminders_due_flag`
    // in tests/objects.rs is what catches them drifting apart.
    //
    // It counts service reminders only. A reading reminder's due date is a calendar-month
    // addition on the latest reading, which SQL cannot spell the same way on both backends, so
    // those are counted in Rust by `due_readings` below with the one copy of the rule.
    // `CAST(SUM(...) AS BIGINT)`, not a bare `SUM`: PostgreSQL widens a sum over a BIGINT
    // column to NUMERIC, which sqlx's `Any` driver cannot decode at all -- every read of an
    // object failed with "Any driver does not support the Postgres type Numeric" before the
    // cast. SQLite reads the cast as INTEGER affinity and is unchanged by it.
    let rows = sqlx::query_as::<_, DerivedRow>(
        "SELECT o.id AS object_id, \
           COALESCE(CAST((SELECT SUM(cost_cents) FROM activities WHERE object_id = o.id AND deleted_at IS NULL) AS BIGINT), 0) AS total_cost_cents, \
           (SELECT COUNT(*) FROM activities WHERE object_id = o.id AND deleted_at IS NULL) AS activity_count, \
           (SELECT weight_grams FROM activities WHERE object_id = o.id AND deleted_at IS NULL AND weight_grams IS NOT NULL AND date <= $4 ORDER BY date DESC, created_at DESC, id DESC LIMIT 1) AS latest_weight_grams, \
           (SELECT date FROM activities WHERE object_id = o.id AND deleted_at IS NULL AND weight_grams IS NOT NULL AND date <= $4 ORDER BY date DESC, created_at DESC, id DESC LIMIT 1) AS latest_weight_date, \
           (SELECT MAX(counter_value) FROM activities WHERE object_id = o.id AND deleted_at IS NULL) AS current_counter, \
           (SELECT COUNT(*) FROM reminders r WHERE r.object_id = o.id AND r.done_at IS NULL AND r.deleted_at IS NULL \
              AND r.kind = 'service' \
              AND (r.snoozed_until IS NULL OR r.snoozed_until <= $2) AND ( \
              (r.due_date IS NOT NULL AND r.due_date <= $2) OR \
              (r.due_counter IS NOT NULL AND r.due_counter <= (SELECT MAX(counter_value) FROM activities WHERE object_id = o.id AND deleted_at IS NULL)) \
           )) AS due_reminder_count, \
           (SELECT MAX(date) FROM activities WHERE object_id = o.id AND deleted_at IS NULL \
              AND counter_value IS NOT NULL AND date <= $4) AS last_reading_date, \
           (SELECT MAX(date) FROM activities WHERE object_id = o.id AND deleted_at IS NULL \
              AND date <= $2) AS last_activity_date, \
           (SELECT file_id FROM attachments WHERE id = o.cover_attachment_id AND deleted_at IS NULL) AS cover_file_id \
         FROM objects o WHERE o.deleted_at IS NULL AND ($1 IS NULL OR o.user_id = $1) AND ($3 IS NULL OR o.id = $3)",
    )
    .bind(user_id).bind(db::today()).bind(only).bind(super::reminders::reading_horizon())
    .fetch_all(&state.db).await?;
    let mut rows: HashMap<i64, DerivedRow> = rows.into_iter().map(|r| (r.object_id, r)).collect();
    for (object_id, due) in due_readings(state, user_id, only).await? {
        if let Some(row) = rows.get_mut(&object_id) {
            row.due_reminder_count += due;
        }
    }
    // One query for every object in scope, the same rate reminders and the Info tab use.
    for (object_id, usage) in super::insights::usage_by_object(state, user_id, only).await? {
        if let Some(row) = rows.get_mut(&object_id) {
            row.counter_per_day_milli = Some(usage.rate_milli);
        }
    }
    Ok(rows)
}

/// How many reading reminders are due per object, for the same scope `derived` answers.
/// Decided by `domain::reminder::reading_status`, the rule each reminder's own `due` uses.
async fn due_readings(
    state: &App,
    user_id: Option<i64>,
    only: Option<i64>,
) -> Result<HashMap<i64, i64>, AppError> {
    use crate::domain::reminder::{reading_status, CalendarSchedule, Every};
    type ReadingRow = (
        i64,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let today_str = db::today();
    let rows: Vec<ReadingRow> = sqlx::query_as(
        "SELECT r.object_id, r.due_date, r.every_n, r.every_unit, r.snoozed_until, r.schedule, \
           (SELECT MAX(a.date) FROM activities a WHERE a.object_id = r.object_id AND a.deleted_at IS NULL \
              AND ((o.type = 'body' AND a.weight_grams IS NOT NULL) OR (o.type <> 'body' AND a.counter_value IS NOT NULL)) AND a.date <= $2) AS last_reading_date \
         FROM reminders r JOIN objects o ON o.id = r.object_id \
         WHERE r.kind = 'reading' AND r.done_at IS NULL AND r.deleted_at IS NULL AND o.deleted_at IS NULL \
           AND ($1 IS NULL OR o.user_id = $1) AND ($3 IS NULL OR o.id = $3)",
    )
    .bind(user_id).bind(super::reminders::reading_horizon()).bind(only)
    .fetch_all(&state.db).await?;
    let parse = |s: &Option<String>| {
        s.as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    };
    let Some(today) = parse(&Some(today_str.clone())) else {
        return Ok(HashMap::new());
    };
    let mut counts = HashMap::new();
    for (object_id, start, every_n, every_unit, snoozed, schedule, last) in rows {
        let every = schedule.as_deref().and_then(CalendarSchedule::parse).map(Every::Calendar)
            .or_else(|| Every::from_parts(every_n, every_unit.as_deref()));
        if reading_status(today, parse(&start), parse(&last), every, parse(&snoozed)).0 {
            *counts.entry(object_id).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

impl DerivedRow {
    fn stats(&self) -> ObjectStats {
        ObjectStats {
            total_cost_cents: self.total_cost_cents,
            activity_count: self.activity_count,
            current_counter: self.current_counter,
            latest_weight_grams: self.latest_weight_grams,
            latest_weight_date: self.latest_weight_date.clone(),
            due_reminder_count: self.due_reminder_count,
            last_reading_date: self.last_reading_date.clone(),
            last_activity_date: self.last_activity_date.clone(),
            counter_per_day_milli: self.counter_per_day_milli,
        }
    }

    fn into_out(self, object: ObjectRow) -> ObjectOut {
        let stats = self.stats();
        ObjectOut {
            object,
            stats,
            cover_file_id: self.cover_file_id,
            ancestors: Vec::new(),
        }
    }
}

/// The stats block for one object, for callers outside this module. Ownership is not checked
/// here: every caller has already loaded the object through `load_owned_object`.
pub async fn stats(state: &App, object_id: i64) -> Result<ObjectStats, AppError> {
    derived(state, None, Some(object_id))
        .await?
        .get(&object_id)
        .map(DerivedRow::stats)
        .ok_or(AppError::NotFound)
}

/// The object's ancestor chain, root first, excluding the object itself -- empty when it has
/// no parent, which costs nothing extra: the common case (an object with no parent) never
/// reaches the recursive query at all.
///
/// The walk carries no `user_id` and no `deleted_at` filter, and that is deliberate rather than
/// an oversight -- do not "fix" it here, and do not copy the pattern to a query reached any
/// other way. Three things together are what make it safe, and all three are about the caller:
/// the only caller is `with_stats`, whose row always came from `load_owned_object` (or
/// `load_owned_object_on`), which filters both; every write to `parent_id` goes through
/// `record::parent_is_valid`, which refuses a parent that is not the same user's and undeleted;
/// and `record::cascade_object` tombstones a whole subtree at once, so no live child can outlive
/// a tombstoned ancestor. A chain reached from an object the caller owns is therefore made
/// entirely of undeleted objects the caller owns, and re-filtering would only cost a join. A
/// caller that cannot make all three claims needs the filters.
///
/// Plain `UNION`, not `UNION ALL`, for the same reason `record::parent_is_valid` uses it: a
/// cycle can never be created through either door, but if one ever got into the data anyway
/// `UNION ALL` would walk it forever and hang the read.
///
/// What makes the `UNION` actually terminate is that the CTE projects nothing but
/// `(id, name, parent_id)` -- no depth, no step counter, nothing that distinguishes a
/// revisited row from its first visit. `UNION` deduplicates whole *rows*, so a column that
/// counted the walk would make every row unique by construction and silently defeat the dedup
/// the cycle defence rests on. That is exactly what an earlier version of this query did. The
/// ordering the caller needs is therefore reconstructed in Rust below instead of asked of SQL,
/// and the `remove` that reconstructs it is a second, independent stop: an ancestor already
/// consumed cannot be walked to twice, so no residual data anomaly can spin the Rust loop
/// either, whatever the database returned.
async fn ancestors(state: &App, object: &ObjectRow) -> Result<Vec<(i64, String)>, AppError> {
    let Some(parent_id) = object.parent_id else {
        return Ok(Vec::new());
    };
    let rows: Vec<(i64, String, Option<i64>)> = sqlx::query_as(
        "WITH RECURSIVE chain(id, name, parent_id) AS ( \
           SELECT id, name, parent_id FROM objects WHERE id = $1 \
           UNION \
           SELECT o.id, o.name, o.parent_id FROM objects o \
             JOIN chain c ON o.id = c.parent_id \
         ) SELECT id, name, parent_id FROM chain WHERE id != $1",
    )
    .bind(object.id)
    .fetch_all(&state.db)
    .await?;

    // The query above is unordered -- a set, not a path -- so the chain is rebuilt by following
    // `parent_id` from the object outwards, which yields nearest ancestor first, then reversed
    // for the root-first order the breadcrumb wants.
    let mut by_id: HashMap<i64, (String, Option<i64>)> = rows
        .into_iter()
        .map(|(id, name, parent_id)| (id, (name, parent_id)))
        .collect();
    let mut chain = Vec::with_capacity(by_id.len());
    let mut next = Some(parent_id);
    while let Some(id) = next {
        let Some((name, parent_id)) = by_id.remove(&id) else {
            break;
        };
        next = parent_id;
        chain.push((id, name));
    }
    chain.reverse();
    Ok(chain)
}

async fn with_stats(state: &App, object: ObjectRow) -> Result<ObjectOut, AppError> {
    let id = object.id;
    let chain = ancestors(state, &object).await?;
    let mut out = derived(state, Some(object.user_id), Some(id))
        .await?
        .remove(&id)
        .map(|d| d.into_out(object))
        .ok_or(AppError::NotFound)?;
    out.ancestors = chain
        .into_iter()
        .map(|(id, name)| Ancestor { id, name })
        .collect();
    Ok(out)
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub archived: bool,
    /// Absent lists roots only -- the dashboard's contract. An id lists that object's direct
    /// children.
    #[serde(default)]
    pub parent_id: Option<i64>,
    /// Every object the user owns, regardless of nesting, still subject to `archived`.
    #[serde(default)]
    pub all: bool,
}

async fn list(
    user: AuthUser,
    State(state): State<App>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ObjectOut>>, AppError> {
    // `COLLATE NOCASE` is SQLite's spelling; PostgreSQL sorts by `lower(name)`. Without it a
    // list reads as "Banana, apple, cherry", which looks like a bug to the person who typed
    // the names. See `dialect::Backend::name_order`.
    let order = state.backend.name_order("name");
    let archived = if q.archived { "IS NOT NULL" } else { "IS NULL" };
    let rows = sqlx::query_as::<_, ObjectRow>(sqlx::AssertSqlSafe(format!(
        "SELECT id, user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags, \
         energy_price_milli, weight_unit, fuel_capacity_milli, resource_unit, resource_kind, measurement_mode, monthly_target_milli, low_level_pct, private \
         FROM objects WHERE user_id = $1 AND deleted_at IS NULL AND archived_at {archived} \
           AND ($3 OR ($2 IS NULL AND parent_id IS NULL) OR (parent_id = $2)) \
         ORDER BY {order}")))
        .bind(user.id).bind(q.parent_id).bind(q.all).fetch_all(&state.db).await?;
    // Empty archived/child lists need no aggregates across the entire account.
    if rows.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let mut derived = derived(&state, Some(user.id), None).await?;
    let out = rows
        .into_iter()
        .filter_map(|row| derived.remove(&row.id).map(|d| d.into_out(row)))
        .collect();
    Ok(Json(out))
}

async fn create(
    user: AuthUser,
    State(state): State<App>,
    Json(mut body): Json<ObjectInput>,
) -> Result<(StatusCode, Json<ObjectOut>), AppError> {
    body.validate()?;
    let client_uuid = super::normalize_client_uuid(body.client_uuid.take())?;
    if let Some(uuid) = client_uuid.as_deref() {
        // Idempotent on the caller's own live row; a conflict on anyone else's or on a
        // tombstone. Checked outside the write transaction on purpose: a hit answers without
        // ever taking the write lock, and a miss that races another replay trips the unique
        // index on `client_uuid` below, which is answered the same way.
        let existing: Option<(i64, i64, Option<String>)> =
            sqlx::query_as("SELECT id, user_id, deleted_at FROM objects WHERE client_uuid = $1")
                .bind(uuid)
                .fetch_optional(&state.db)
                .await?;
        match existing {
            Some((id, owner, None)) if owner == user.id => {
                // By design a replay answers with the row the first attempt created, before the type and parent checks below.
                let row = load_owned_object(&state, user.id, id).await?;
                return Ok((StatusCode::OK, Json(with_stats(&state, row).await?)));
            }
            Some(_) => return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into())),
            None => {}
        }
    }
    let now = db::now();
    let archived_at = if body.archived == Some(true) {
        Some(now.clone())
    } else {
        None
    };
    let object_uuid = client_uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    check_type(&mut tx, user.id, &body.type_).await?;
    // Checked inside the transaction, on its connection -- never from the pool -- for the two
    // reasons spelled out on `update`: a pool connection taken while `begin_write` holds the
    // write lock deadlocks on SQLite, and a check taken before the lock can go stale before
    // the write it guards lands. `flatten()` collapses "absent" and an explicit `null` to the
    // same `None`: on create there is no existing value for the two to mean different things
    // about.
    let parent_id = body.parent_id.flatten();
    if let Some(pid) = parent_id {
        if !record::parent_is_valid(&mut tx, user.id, None, pid).await? {
            return Err(AppError::BadRequest(PARENT_REJECTION.into()));
        }
    }
    // Omitted and `null` both mean "no price" on create, same as `parent_id` above.
    let energy_price_milli = body.energy_price_milli.flatten();
    let fuel_capacity_milli = body.fuel_capacity_milli.flatten();
    // The presence of the neutral fields distinguishes a new resource-aware client from a
    // legacy client that only knows `fuel_unit`. Keep legacy vehicle charging on the `fuel`
    // category; silently inferring a resource kind would change its existing UI and sync shape.
    let uses_resource_model = body.resource_unit.is_some()
        || body.resource_kind.is_some()
        || body.measurement_mode.is_some();
    let resource_unit = body
        .resource_unit
        .clone()
        .flatten()
        .or_else(|| body.fuel_unit.clone());
    let resource_kind = body.resource_kind.clone().flatten().or_else(|| {
        uses_resource_model.then(|| match resource_unit.as_deref() {
                Some("kwh") => Some("electricity".into()),
                Some("l" | "gal") if body.type_ == "home" => Some("heating_fuel".into()),
                Some("l" | "gal") => Some("vehicle_fuel".into()),
                _ => None,
            }).flatten()
    });
    let measurement_mode = body
        .measurement_mode
        .clone()
        .flatten()
        .or_else(|| uses_resource_model.then(|| resource_unit.as_ref().map(|_| "usage".into())).flatten());
    if fuel_capacity_milli.is_some() && !matches!(resource_unit.as_deref(), Some("l") | Some("gal"))
    {
        return Err(AppError::BadRequest(
            "fuel_capacity_milli needs a liquid fuel unit".into(),
        ));
    }
    let legacy_fuel_unit = resource_unit
        .as_ref()
        .filter(|u| u.as_str() != "m3")
        .cloned();
    let row = sqlx::query_as::<_, ObjectRow>(
        "INSERT INTO objects (user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags, \
         energy_price_milli, weight_unit, fuel_capacity_milli, resource_unit, resource_kind, measurement_mode, monthly_target_milli, low_level_pct, private) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23) \
         RETURNING id, user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags, \
         energy_price_milli, weight_unit, fuel_capacity_milli, resource_unit, resource_kind, measurement_mode, monthly_target_milli, low_level_pct, private",
    )
    .bind(user.id).bind(&body.name).bind(&body.type_).bind(&body.counter_unit).bind(&legacy_fuel_unit).bind(&body.description)
    .bind(&body.purchase_date).bind(body.purchase_price_cents).bind(archived_at).bind(parent_id).bind(&now).bind(&now)
    .bind(&object_uuid).bind(tags::to_json(body.tags.as_deref().unwrap_or_default()))
    .bind(energy_price_milli).bind(body.weight_unit.as_deref().unwrap_or("kg")).bind(fuel_capacity_milli)
    .bind(&resource_unit).bind(&resource_kind).bind(&measurement_mode).bind(body.monthly_target_milli.flatten())
    .bind(body.low_level_pct.flatten()).bind(i64::from(body.private.unwrap_or(false)))
    .fetch_one(&mut *tx).await;
    let row = match row {
        Ok(row) => row,
        // Two replays of one client_uuid racing past the pre-check above: the loser trips the
        // unique index on `client_uuid`. The id is spoken for, so that is the same conflict.
        Err(e)
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation()) =>
        {
            tx.rollback().await?;
            return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into()));
        }
        Err(e) => return Err(e.into()),
    };
    record::record_create(&mut tx, user.id, Entity::Object, &object_uuid, &edited_at).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(with_stats(&state, row).await?)))
}

async fn read(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<Json<ObjectOut>, AppError> {
    let row = load_owned_object(&state, user.id, id).await?;
    Ok(Json(with_stats(&state, row).await?))
}

async fn update(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
    Json(mut body): Json<ObjectInput>,
) -> Result<Json<ObjectOut>, AppError> {
    // Request-shape validation first, and it is the only thing that happens before the write
    // lock: it reads no database state at all, so a malformed body can be answered 400 without
    // stalling every other writer in the instance for the length of a transaction.
    body.validate()?;

    let mut tx = db::begin_write(&state.db, state.backend).await?;

    // EVERY read this handler makes is made here, inside the transaction holding the write
    // lock, and on that transaction's own connection. Both halves of that are load-bearing.
    //
    // Inside, because a PATCH does not write only what the client sent: `archived_at`,
    // `cover_attachment_id` and `parent_id` are each *carried over* from `existing` when the
    // client omits the field, so a value read before the lock was granted is written back
    // afterwards as though the client had asked for it. For `parent_id` that is not merely a
    // lost update, it is a corrupt tree. Start with `A.parent_id = P`, and let three requests
    // overlap (the pool is 4 on SQLite, 16 on PostgreSQL, so they do):
    //
    //   1. `PATCH /objects/A {"name": "X"}` -- no `parent_id` key -- reads `A.parent_id = P`,
    //      then waits for the lock.
    //   2. `PATCH /objects/A {"parent_id": null}` commits. `A` is a root.
    //   3. `PATCH /objects/P {"parent_id": A}` commits: `A`'s ancestors are just `{A}`, so the
    //      cycle check passes honestly.
    //   4. Request 1 wakes and writes the parent it read in step 1.
    //
    // `A.parent_id = P` and `P.parent_id = A`: a cycle that `record::parent_is_valid` was never
    // asked about, because request 1 sent no parent to validate. Nothing in the app can see it
    // (both rows drop out of the root-only list), nothing purges it (the guard in `sync::feed`
    // holds each row back for the other, forever, silently) and `--copy-to` cannot order the
    // pair, so the documented SQLite-to-PostgreSQL migration aborts on the foreign key. The
    // shipped PWA always sends `parent_id`; a hand-written client against the bearer-token API
    // is exactly what omits an optional field.
    //
    // On `tx`'s connection, because a second pool connection acquired while `begin_write` holds
    // SQLite's write lock does not fail, it hangs.
    let existing = load_owned_object_on(&mut tx, user.id, id).await?;
    check_type(&mut tx, user.id, &body.type_).await?;
    // `km` and `mi` may always trade places; only a change that leaves that pair (to `h`, or to
    // no counter at all) is checked against the object's trips -- see
    // `COUNTER_UNIT_TRIP_REJECTION`'s doc comment for why one may never exist without the other.
    if !matches!(body.counter_unit.as_deref(), Some("km") | Some("mi")) {
        let has_trip: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM activities WHERE object_id = $1 AND category = 'trip' AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if has_trip.is_some() {
            return Err(AppError::BadRequest(COUNTER_UNIT_TRIP_REJECTION.into()));
        }
    }
    let resource_unit = match body.resource_unit.clone() {
        Some(value) => value,
        // A legacy client has no `resource_unit` key and resends `fuel_unit` in full, including
        // null when clearing it. Honour that shape; new clients always send `resource_unit`.
        None => body.fuel_unit.clone(),
    };
    if resource_unit != existing.resource_unit {
        let has_fuel_history: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM activities WHERE object_id = $1 AND deleted_at IS NULL AND (quantity_milli IS NOT NULL OR fuel_level_pct IS NOT NULL) LIMIT 1",
        )
        .bind(id).fetch_optional(&mut *tx).await?;
        if has_fuel_history.is_some() {
            return Err(AppError::BadRequest(FUEL_UNIT_HISTORY_REJECTION.into()));
        }
    }
    let archived_at = match body.archived {
        Some(true) => existing.archived_at.clone().or_else(|| Some(db::now())),
        Some(false) => None,
        None => existing.archived_at.clone(),
    };
    let cover_attachment_id = match body.cover_attachment_id {
        None => existing.cover_attachment_id,
        Some(None) => None,
        Some(Some(cover)) => {
            let ok: Option<(i64,)> = sqlx::query_as("SELECT id FROM attachments WHERE id = $1 AND object_id = $2 AND kind = 'photo' AND deleted_at IS NULL")
                .bind(cover).bind(id).fetch_optional(&mut *tx).await?;
            if ok.is_none() {
                return Err(AppError::BadRequest(
                    "cover_attachment_id must be a photo of this object".into(),
                ));
            }
            Some(cover)
        }
    };
    // The cycle check, on the same connection and under the same lock, so a tree that passes it
    // here is still that tree when the `UPDATE` below lands. `sync::apply`'s `Set` path asks
    // `record::parent_is_valid` the identical question, from inside its own `begin_write`.
    let parent_id = match body.parent_id {
        None => existing.parent_id,
        Some(None) => None,
        Some(Some(pid)) => {
            if !record::parent_is_valid(&mut tx, user.id, Some(id), pid).await? {
                return Err(AppError::BadRequest(PARENT_REJECTION.into()));
            }
            Some(pid)
        }
    };
    // A price omitted on PATCH keeps its stored value (three-state, like `cover_attachment_id`
    // above) -- so the other half of `ObjectInput::validate`'s rule, which only ever sees an
    // explicit price in the body, cannot by itself catch a PATCH that clears `fuel_unit` while
    // leaving an already-stored price untouched. Checked here instead, against the row this
    // transaction is holding the write lock on -- `sync::apply`'s `Set` path asks the identical
    // question, from inside its own `begin_write`, for the same reason `parent_id`'s cycle
    // check above does.
    let energy_price_milli = match body.energy_price_milli {
        None => existing.energy_price_milli,
        Some(v) => v,
    };
    if energy_price_milli.is_some() && resource_unit.is_none() {
        return Err(AppError::BadRequest(ENERGY_PRICE_NEEDS_FUEL_UNIT.into()));
    }
    let fuel_capacity_milli = match body.fuel_capacity_milli {
        None => existing.fuel_capacity_milli,
        Some(v) => v,
    };
    if fuel_capacity_milli.is_some() && !matches!(resource_unit.as_deref(), Some("l") | Some("gal"))
    {
        return Err(AppError::BadRequest(
            "fuel_capacity_milli needs a liquid fuel unit".into(),
        ));
    }
    let resource_kind = body
        .resource_kind
        .clone()
        .unwrap_or(existing.resource_kind.clone());
    let measurement_mode = body
        .measurement_mode
        .clone()
        .unwrap_or(existing.measurement_mode.clone());
    let monthly_target_milli = body
        .monthly_target_milli
        .unwrap_or(existing.monthly_target_milli);
    let low_level_pct = body.low_level_pct.unwrap_or(existing.low_level_pct);
    let private = body.private.map(i64::from).unwrap_or(existing.is_private);
    let legacy_fuel_unit = resource_unit
        .as_ref()
        .filter(|u| u.as_str() != "m3")
        .cloned();

    // Only fields whose value actually differs are logged -- see `record::record_update` --
    // so a PATCH that rewrites a field with its existing value produces no `changes` row.
    // Every value is settled before this point, `parent_id` included, so the diff and the
    // single `UPDATE` below always agree about what is being written.
    let mut changed: Vec<(&str, serde_json::Value)> = Vec::new();
    if let Some(unit) = &body.weight_unit {
        if unit != &existing.weight_unit {
            changed.push(("weight_unit", json!(unit)));
        }
    }
    if body.name != existing.name {
        changed.push(("name", json!(body.name)));
    }
    if body.type_ != existing.type_ {
        changed.push(("type", json!(body.type_)));
    }
    if body.counter_unit != existing.counter_unit {
        changed.push(("counter_unit", json!(body.counter_unit)));
    }
    if legacy_fuel_unit != existing.fuel_unit {
        changed.push(("fuel_unit", json!(legacy_fuel_unit)));
    }
    if resource_unit != existing.resource_unit {
        changed.push(("resource_unit", json!(resource_unit)));
    }
    if resource_kind != existing.resource_kind {
        changed.push(("resource_kind", json!(resource_kind)));
    }
    if measurement_mode != existing.measurement_mode {
        changed.push(("measurement_mode", json!(measurement_mode)));
    }
    if monthly_target_milli != existing.monthly_target_milli {
        changed.push(("monthly_target_milli", json!(monthly_target_milli)));
    }
    if low_level_pct != existing.low_level_pct {
        changed.push(("low_level_pct", json!(low_level_pct)));
    }
    if private != existing.is_private {
        changed.push(("private", json!(private)));
    }
    if body.description != existing.description {
        changed.push(("description", json!(body.description)));
    }
    if body.purchase_date != existing.purchase_date {
        changed.push(("purchase_date", json!(body.purchase_date)));
    }
    if body.purchase_price_cents != existing.purchase_price_cents {
        changed.push(("purchase_price_cents", json!(body.purchase_price_cents)));
    }
    if archived_at != existing.archived_at {
        changed.push(("archived_at", json!(archived_at)));
    }
    if cover_attachment_id != existing.cover_attachment_id {
        changed.push(("cover_attachment_id", json!(cover_attachment_id)));
    }
    if parent_id != existing.parent_id {
        changed.push(("parent_id", json!(parent_id)));
    }
    // Absent keeps the stored tags. The change is logged as the JSON text the column holds, the
    // same shape a sync `set` op carries for any other text field.
    let tags = body
        .tags
        .as_deref()
        .map(tags::to_json)
        .unwrap_or_else(|| existing.tags.clone());
    if tags != existing.tags {
        changed.push(("tags", json!(tags)));
    }
    if energy_price_milli != existing.energy_price_milli {
        changed.push(("energy_price_milli", json!(energy_price_milli)));
    }
    if fuel_capacity_milli != existing.fuel_capacity_milli {
        changed.push(("fuel_capacity_milli", json!(fuel_capacity_milli)));
    }

    sqlx::query(
        "UPDATE objects SET name = $1, type = $2, counter_unit = $3, fuel_unit = $4, description = $5, purchase_date = $6, \
         purchase_price_cents = $7, archived_at = $8, cover_attachment_id = $9, parent_id = $10, updated_at = $11, tags = $12, \
         energy_price_milli = $13, weight_unit = $14, fuel_capacity_milli = $15, resource_unit = $16, resource_kind = $17, \
         measurement_mode = $18, monthly_target_milli = $19, low_level_pct = $20, private = $21 \
         WHERE id = $22 AND deleted_at IS NULL",
    )
    .bind(&body.name).bind(&body.type_).bind(&body.counter_unit).bind(&legacy_fuel_unit).bind(&body.description)
    .bind(&body.purchase_date).bind(body.purchase_price_cents).bind(&archived_at)
    .bind(cover_attachment_id).bind(parent_id).bind(db::now()).bind(&tags)
    .bind(energy_price_milli).bind(body.weight_unit.as_deref().unwrap_or(&existing.weight_unit)).bind(fuel_capacity_milli)
    .bind(&resource_unit).bind(&resource_kind).bind(&measurement_mode).bind(monthly_target_milli).bind(low_level_pct).bind(private).bind(id)
    .execute(&mut *tx).await?;
    if !changed.is_empty() {
        let uuid = record::uuid_of(&mut tx, Entity::Object, id).await?;
        record::record_update(
            &mut tx,
            user.id,
            Entity::Object,
            &uuid,
            &changed,
            &record::edited_at_now(),
        )
        .await?;
    }
    tx.commit().await?;

    let row = load_owned_object(&state, user.id, id).await?;
    Ok(Json(with_stats(&state, row).await?))
}

/// Deleting an object writes a tombstone rather than removing the row, so a client that was
/// offline when the delete happened can still learn about it on its next sync.
///
/// `ON DELETE CASCADE` only fires for a real `DELETE`, so the cascade the schema used to
/// provide has to be spelled out here -- without it the object's activities, reminders and
/// attachments would stay alive and keep syncing after their parent was gone. One transaction,
/// so a half-applied cascade cannot survive a failure mid-way.
///
/// The object's `files` rows and their blobs are deliberately left alone: a file is
/// content-addressed and shared, `attachments.file_id` is `ON DELETE RESTRICT`, and the
/// attachment rows pointing at it still exist. They are freed when the retention purge
/// finally removes those tombstoned attachments.
async fn delete(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let now = db::now();
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let affected = sqlx::query(
        "UPDATE objects SET deleted_at = $1, updated_at = $2 \
         WHERE id = $3 AND user_id = $4 AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    // `record::cascade_object` is the single copy of this cascade, shared with
    // `sync::apply::apply_op`'s `delete` handling -- see the module docs on `sync::record` for
    // why hand-rolling it a second time here is exactly what drifted twice before.
    let object_uuid = record::uuid_of(&mut tx, Entity::Object, id).await?;
    let cascaded = record::cascade_object(&mut tx, &object_uuid, &now).await?;
    record::record_delete(&mut tx, user.id, Entity::Object, &object_uuid, &edited_at).await?;
    record::log_cascade(&mut tx, user.id, &edited_at, record::DEVICE_ID, &cascaded).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
