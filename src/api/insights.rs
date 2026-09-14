use super::objects::load_owned_object;
use crate::auth::AuthUser;
use crate::db;
use crate::domain::insights::{
    consumption_per_100_milli, cost_per_counter_milli, daily_rate_milli, default_fuel_unit, fuel_cost_per_counter_milli,
    latest_reading, monthly_usage, Fill, MonthUsage, Reading, RATE_WINDOW_DAYS,
};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use serde::Serialize;
use std::collections::HashMap;

pub fn router() -> Router<App> {
    Router::new().route("/objects/{id}/insights", get(read))
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
}

/// How many months the usage chart covers.
const USAGE_MONTHS: u32 = 12;

/// An object's newest reading and the rate its recent readings rise at.
#[derive(Clone, Copy, Debug)]
pub struct Usage {
    pub last: Reading,
    pub rate_milli: i64,
}

/// Usage for every object of `user_id` that has enough readings for a rate, or just `only`.
///
/// One query over the readings of the last window and a bit -- the fallback in
/// `daily_rate_milli` reaches past the window, so this reads twice its length -- rather than one
/// per object, because the dashboard's lookahead asks for every object at once. Readings dated
/// past `reminders::reading_horizon` are left out: a typo'd year must not become the "latest"
/// reading.
pub async fn usage_by_object(state: &App, user_id: i64, only: Option<i64>) -> Result<HashMap<i64, Usage>, AppError> {
    let today = db::today();
    let from = NaiveDate::parse_from_str(&today, "%Y-%m-%d")
        .map(|t| (t - chrono::Duration::days(RATE_WINDOW_DAYS * 2)).to_string())
        .unwrap_or_else(|_| today.clone());
    let rows: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT a.object_id, a.date, a.counter_value FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND ($2 IS NULL OR o.id = $2) AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
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

async fn read(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
) -> Result<Json<InsightsOut>, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;

    // Every `SUM` here is cast back to BIGINT: PostgreSQL widens a sum over a BIGINT column to
    // NUMERIC, which sqlx's `Any` driver cannot decode. SQLite is unaffected by the cast.
    let by_year = sqlx::query_as::<_, Bucket>(
        "SELECT substr(date, 1, 4) AS bucket, COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id = $1 AND deleted_at IS NULL \
         GROUP BY bucket ORDER BY bucket DESC",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;

    // Readings are left out: they never carry a cost, and a monthly reading habit would put
    // an empty "Reading" bar at the bottom of every car's breakdown.
    let by_category = sqlx::query_as::<_, Bucket>(
        "SELECT category AS bucket, COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id = $1 AND deleted_at IS NULL AND category <> 'reading' \
         GROUP BY category ORDER BY cost_cents DESC",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;

    let (min_counter, max_counter, total_cost): (Option<i64>, Option<i64>, i64) = sqlx::query_as(
        "SELECT MIN(counter_value), MAX(counter_value), COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) \
         FROM activities WHERE object_id = $1 AND deleted_at IS NULL",
    )
    .bind(object_id)
    .fetch_one(&state.db)
    .await?;

    let counter_span = min_counter.zip(max_counter).map(|(from, to)| Span { from, to });
    let span = counter_span.as_ref().map(|s| s.to - s.from).unwrap_or(0);
    let overall_cost_per_counter_milli = cost_per_counter_milli(total_cost, span);

    let fill_rows: Vec<(i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT counter_value, quantity_milli, cost_cents FROM activities \
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
            .map(|(counter, quantity_milli, cost_cents)| Fill {
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
        })
    };

    let counter_per_day_milli =
        usage_by_object(&state, user.id, Some(object_id)).await?.get(&object_id).map(|u| u.rate_milli);

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
        let today = NaiveDate::parse_from_str(&db::today(), "%Y-%m-%d").expect("server-generated date is always valid");
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
    }))
}
