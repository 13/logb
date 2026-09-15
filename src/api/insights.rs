use super::objects::load_owned_object;
use crate::auth::AuthUser;
use crate::db;
use crate::domain::insights::{
    consumption_per_100_milli, consumption_per_fill, cost_per_counter_milli, daily_rate_milli, default_fuel_unit,
    fuel_cost_per_counter_milli, latest_reading, monthly_usage, DatedFill, Fill, FillRate, MonthUsage, Reading, RATE_WINDOW_DAYS,
};
use crate::domain::stats::{self, day_of, months_ending, purchase_spend, Amount, Ownership};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects/{id}/insights", get(read))
        .route("/objects/{id}/usage", get(usage))
}

#[derive(Serialize)]
pub struct UsageOut {
    /// Counter units per day over recent readings, scaled by 1000 -- the same figure as
    /// `InsightsOut::counter_per_day_milli`. Null until there is enough history.
    pub counter_per_day_milli: Option<i64>,
}

/// Just the rate. The reading form needs it to question an implausible reading, and asking
/// `/insights` for it meant running every cost rollup on each open of a one-field form.
async fn usage(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>) -> Result<Json<UsageOut>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let counter_per_day_milli =
        usage_by_object(&state, Some(user.id), Some(object_id)).await?.get(&object_id).map(|u| u.rate_milli);
    Ok(Json(UsageOut { counter_per_day_milli }))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Bucket {
    pub bucket: String,
    pub cost_cents: i64,
    pub count: i64,
}

#[derive(Serialize)]
pub struct Span {
    pub from: i64,
    pub to: i64,
}

#[derive(Serialize)]
pub struct FuelOut {
    pub unit: String,
    pub quantity_milli: i64,
    pub per_100_milli: Option<i64>,
    pub cost_per_counter_milli: Option<i64>,
    /// Consumption per fill, oldest first -- see `domain::insights::consumption_per_fill`.
    pub fills: Vec<FillRate>,
}

#[derive(Serialize)]
pub struct InsightsOut {
    pub by_year: Vec<Bucket>,
    pub by_category: Vec<Bucket>,
    pub counter_span: Option<Span>,
    pub cost_per_counter_milli: Option<i64>,
    pub fuel: Option<FuelOut>,
    /// Counter units per day over recent readings, scaled by 1000 -- see
    /// `domain::insights::daily_rate_milli`. Null until there is enough history.
    pub counter_per_day_milli: Option<i64>,
    /// The last twelve calendar months, oldest first -- see `domain::insights::monthly_usage`.
    /// Empty for an object without a counter, or without a single measurable month.
    pub usage_by_month: Vec<MonthUsage>,
    /// Whether the object has any non-deleted child, so a client knows if `contents` would change anything.
    pub has_contents: bool,
    pub ownership: Ownership,
    /// The last twelve calendar months of spend, oldest first, zeros included.
    pub by_month: Vec<Amount>,
    /// The same twelve calendar months as `by_month` (and `usage_by_month`), oldest first, of
    /// trip distance -- see `domain::trips`. Zero for a month with no trip, never hidden.
    pub trip_distance_by_month: Vec<MonthDistance>,
}

/// One month of `InsightsOut::trip_distance_by_month`.
#[derive(Serialize)]
pub struct MonthDistance {
    /// `YYYY-MM`.
    pub month: String,
    pub distance: i64,
}

/// How many months the usage chart covers.
const USAGE_MONTHS: u32 = 12;

/// An object's newest reading and the rate its recent readings rise at.
#[derive(Clone, Copy, Debug)]
pub struct Usage {
    pub last: Reading,
    pub rate_milli: i64,
}

/// Usage for every object of `user_id` (or of anyone, for a caller that has already checked
/// ownership of `only`) that has enough readings for a rate, or just `only`.
///
/// One query over the readings of the last window and a bit -- the fallback in
/// `daily_rate_milli` reaches past the window, so this reads twice its length -- rather than one
/// per object, because the dashboard's lookahead asks for every object at once. Readings dated
/// past `reminders::reading_horizon` are left out: a typo'd year must not become the "latest"
/// reading.
pub async fn usage_by_object(state: &App, user_id: Option<i64>, only: Option<i64>) -> Result<HashMap<i64, Usage>, AppError> {
    let today = db::today();
    let from = NaiveDate::parse_from_str(&today, "%Y-%m-%d")
        .map(|t| (t - chrono::Duration::days(RATE_WINDOW_DAYS * 2)).to_string())
        .unwrap_or_else(|_| today.clone());
    let rows: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT a.object_id, a.date, a.counter_value FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE ($1 IS NULL OR o.user_id = $1) AND ($2 IS NULL OR o.id = $2) AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
           AND a.counter_value IS NOT NULL AND a.date <= $3 AND a.date >= $4",
    )
    .bind(user_id).bind(only).bind(super::reminders::reading_horizon()).bind(&from)
    .fetch_all(&state.db)
    .await?;

    let mut readings: HashMap<i64, Vec<Reading>> = HashMap::new();
    for (object_id, date, counter) in rows {
        // A stored date that does not parse is skipped, as every other read of a row does.
        if let Ok(date) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
            readings.entry(object_id).or_default().push(Reading { date, counter });
        }
    }
    Ok(readings
        .into_iter()
        .filter_map(|(object_id, list)| {
            let rate_milli = daily_rate_milli(&list)?;
            Some((object_id, Usage { last: latest_reading(&list)?, rate_milli }))
        })
        .collect())
}

#[derive(Deserialize)]
pub struct InsightsQuery {
    #[serde(default)]
    pub contents: Option<bool>,
}

/// The objects a cost figure covers, as a `WITH` prefix that every cost query below reads through
/// `object_id IN (SELECT id FROM scope)`: the object alone, or with contents the object and every
/// non-deleted descendant. `UNION`, not `UNION ALL`, so even a corrupt parent loop terminates, as
/// in `objects::ancestors`. The root was already checked to be the caller's by
/// `load_owned_object`, and a parent can only ever be set to one of the caller's own objects.
///
/// The recursive step stopping at a deleted child never hides a live grandchild under it:
/// deleting an object tombstones its whole subtree at once (`sync::record::cascade_object`), so a
/// deleted row never has an undeleted child left to miss.
fn scope(contents: bool) -> &'static str {
    if contents {
        "WITH RECURSIVE scope(id) AS ( \
           SELECT id FROM objects WHERE id = $1 \
           UNION \
           SELECT o.id FROM objects o JOIN scope s ON o.parent_id = s.id WHERE o.deleted_at IS NULL \
         ) "
    } else {
        "WITH scope(id) AS (SELECT id FROM objects WHERE id = $1) "
    }
}

async fn read(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
    Query(q): Query<InsightsQuery>,
) -> Result<Json<InsightsOut>, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;
    let scope = scope(q.contents.unwrap_or(false));
    let today = NaiveDate::parse_from_str(&db::today(), "%Y-%m-%d").expect("server-generated date is always valid");

    // Every `SUM` here is cast back to BIGINT: PostgreSQL widens a sum over a BIGINT column to
    // NUMERIC, which sqlx's `Any` driver cannot decode. SQLite is unaffected by the cast.
    let by_year_sql = format!(
        "{scope}SELECT substr(date, 1, 4) AS bucket, COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id IN (SELECT id FROM scope) AND deleted_at IS NULL \
         GROUP BY bucket ORDER BY bucket DESC"
    );
    let by_year = sqlx::query_as::<_, Bucket>(sqlx::AssertSqlSafe(by_year_sql)).bind(object_id).fetch_all(&state.db).await?;

    // Readings are left out: they never carry a cost, and a monthly reading habit would put
    // an empty "Reading" bar at the bottom of every car's breakdown.
    let by_category_sql = format!(
        "{scope}SELECT category AS bucket, COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id IN (SELECT id FROM scope) AND deleted_at IS NULL \
         AND category <> 'reading' GROUP BY category ORDER BY cost_cents DESC"
    );
    let by_category = sqlx::query_as::<_, Bucket>(sqlx::AssertSqlSafe(by_category_sql)).bind(object_id).fetch_all(&state.db).await?;

    let (min_counter, max_counter, min_start_counter, total_cost): (Option<i64>, Option<i64>, Option<i64>, i64) = sqlx::query_as(
        "SELECT MIN(counter_value), MAX(counter_value), MIN(start_counter), \
           COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) \
         FROM activities WHERE object_id = $1 AND deleted_at IS NULL",
    )
    .bind(object_id)
    .fetch_one(&state.db)
    .await?;

    let counter_span = min_counter.zip(max_counter).map(|(from, to)| Span { from, to });
    // A trip's `start_counter` is a real counter reading too, often the earliest one on record
    // (the object's counter was already there before the first trip was ever logged) -- so the
    // cost-per-counter span reaches back to it when it is lower than every plain `counter_value`,
    // rather than understating the span (and so overstating cost-per-unit) by starting only at
    // the first trip's *end*. `counter_span` above is left alone: it already reports the
    // observed counter *readings*, and a trip's start is not a reading a user took, just the
    // value the trip moved off from.
    let span_from = match (min_counter, min_start_counter) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, b) => b,
    };
    let span = span_from.zip(max_counter).map(|(from, to)| to - from).unwrap_or(0);
    let overall_cost_per_counter_milli = cost_per_counter_milli(total_cost, span);

    let running_sql = format!(
        "{scope}SELECT COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) FROM activities \
         WHERE object_id IN (SELECT id FROM scope) AND deleted_at IS NULL"
    );
    let (running_cents,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(running_sql)).bind(object_id).fetch_one(&state.db).await?;

    let scoped_objects_sql = format!(
        "{scope}SELECT id, parent_id, name, type, archived_at, purchase_date, purchase_price_cents, created_at \
         FROM objects WHERE id IN (SELECT id FROM scope)"
    );
    #[allow(clippy::type_complexity)]
    let object_rows: Vec<(i64, Option<i64>, String, String, Option<String>, Option<String>, Option<i64>, String)> =
        sqlx::query_as(sqlx::AssertSqlSafe(scoped_objects_sql)).bind(object_id).fetch_all(&state.db).await?;
    let scoped: Vec<stats::ObjectRow> = object_rows
        .into_iter()
        .map(|(id, parent_id, name, kind, archived_at, purchase_date, purchase_price_cents, created_at)| stats::ObjectRow {
            id, parent_id, name, kind, archived: archived_at.is_some(), purchase_date, purchase_price_cents, created_at,
        })
        .collect();
    // A purchase entry with a cost is the purchase; see `stats::purchase_spend`.
    let purchased_sql = format!(
        "{scope}SELECT DISTINCT object_id FROM activities WHERE object_id IN (SELECT id FROM scope) \
         AND deleted_at IS NULL AND category = 'purchase' AND cost_cents > 0"
    );
    let purchased: Vec<(i64,)> = sqlx::query_as(sqlx::AssertSqlSafe(purchased_sql)).bind(object_id).fetch_all(&state.db).await?;
    let purchased: HashSet<i64> = purchased.into_iter().map(|(id,)| id).collect();
    let purchase_cents: i64 = purchase_spend(&scoped, &purchased).iter().map(|s| s.cost_cents).sum();

    // "Since" and "until" are always the object's own: a boiler bought later does not shorten
    // how long the house has been owned.
    let since = object.purchase_date.as_deref().and_then(day_of).or_else(|| day_of(&object.created_at)).unwrap_or(today);
    let until = object.archived_at.as_deref().and_then(day_of).unwrap_or(today);
    let ownership = stats::ownership(running_cents, purchase_cents, since, until);

    let months_sql = format!(
        "{scope}SELECT substr(date, 1, 7), CAST(SUM(cost_cents) AS BIGINT) FROM activities \
         WHERE object_id IN (SELECT id FROM scope) AND deleted_at IS NULL AND cost_cents IS NOT NULL \
         GROUP BY substr(date, 1, 7)"
    );
    let month_totals: Vec<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(months_sql)).bind(object_id).fetch_all(&state.db).await?;
    let by_month = months_ending(today, USAGE_MONTHS, &month_totals);

    // Trip distance is the object's own, regardless of `contents`, exactly like `usage_by_month`
    // and `counter_per_day_milli` above -- a child's trips are not this object's counter moving.
    // `months_ending` is reused (rather than a bespoke grouping) so this series lines up with
    // `by_month` and `usage_by_month` month for month; its `Amount.cost_cents` field is just
    // renamed to `distance` below, this being a distance total and not a cost one.
    let trip_month_totals: Vec<(String, i64)> = sqlx::query_as(
        "SELECT substr(date, 1, 7), CAST(SUM(counter_value - start_counter) AS BIGINT) FROM activities \
         WHERE object_id = $1 AND deleted_at IS NULL AND category = 'trip' \
         AND start_counter IS NOT NULL AND counter_value IS NOT NULL GROUP BY substr(date, 1, 7)",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;
    let trip_distance_by_month: Vec<MonthDistance> = months_ending(today, USAGE_MONTHS, &trip_month_totals)
        .into_iter()
        .map(|a| MonthDistance { month: a.bucket, distance: a.cost_cents })
        .collect();

    let (children,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects WHERE parent_id = $1 AND deleted_at IS NULL")
        .bind(object_id)
        .fetch_one(&state.db)
        .await?;

    let fill_rows: Vec<(String, i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT date, counter_value, quantity_milli, cost_cents FROM activities \
         WHERE object_id = $1 AND deleted_at IS NULL AND category = 'fuel' AND counter_value IS NOT NULL \
         AND quantity_milli IS NOT NULL ORDER BY counter_value",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;

    let fuel = if fill_rows.is_empty() {
        None
    } else {
        let fills: Vec<Fill> = fill_rows
            .iter()
            .map(|(_, counter, quantity_milli, cost_cents)| Fill {
                counter: *counter,
                quantity_milli: *quantity_milli,
                cost_cents: *cost_cents,
            })
            .collect();
        Some(FuelOut {
            unit: object
                .fuel_unit
                .clone()
                .unwrap_or_else(|| default_fuel_unit(object.counter_unit.as_deref()).to_string()),
            quantity_milli: fills.iter().map(|f| f.quantity_milli).sum(),
            per_100_milli: consumption_per_100_milli(&fills),
            cost_per_counter_milli: fuel_cost_per_counter_milli(&fills),
            fills: consumption_per_fill(
                &fill_rows
                    .iter()
                    .map(|(date, counter, quantity_milli, _)| DatedFill { date: date.clone(), counter: *counter, quantity_milli: *quantity_milli })
                    .collect::<Vec<_>>(),
            ),
        })
    };

    let counter_per_day_milli =
        usage_by_object(&state, Some(user.id), Some(object_id)).await?.get(&object_id).map(|u| u.rate_milli);

    let usage_by_month = if object.counter_unit.is_some() {
        // Every reading, not a window: the first month of the chart is measured from whatever
        // reading came before it, however long ago that was. A household object has a few dozen.
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT date, counter_value FROM activities \
             WHERE object_id = $1 AND deleted_at IS NULL AND counter_value IS NOT NULL AND date <= $2",
        )
        .bind(object_id)
        .bind(super::reminders::reading_horizon())
        .fetch_all(&state.db)
        .await?;
        let readings: Vec<Reading> = rows
            .into_iter()
            .filter_map(|(date, counter)| NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok().map(|date| Reading { date, counter }))
            .collect();
        let months = monthly_usage(&readings, today, USAGE_MONTHS);
        if months.iter().any(|m| m.amount.is_some()) { months } else { Vec::new() }
    } else {
        Vec::new()
    };

    Ok(Json(InsightsOut {
        by_year,
        by_category,
        counter_span,
        cost_per_counter_milli: overall_cost_per_counter_milli,
        fuel,
        counter_per_day_milli,
        usage_by_month,
        has_contents: children > 0,
        ownership,
        by_month,
        trip_distance_by_month,
    }))
}
