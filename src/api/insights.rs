use super::objects::load_owned_object;
use crate::auth::AuthUser;
use crate::domain::insights::{
    consumption_per_100_milli, cost_per_counter_milli, default_fuel_unit, fuel_cost_per_counter_milli, Fill,
};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

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
}

async fn read(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
) -> Result<Json<InsightsOut>, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;

    let by_year = sqlx::query_as::<_, Bucket>(
        "SELECT substr(date, 1, 4) AS bucket, COALESCE(SUM(cost_cents), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id = ? GROUP BY bucket ORDER BY bucket DESC",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;

    let by_category = sqlx::query_as::<_, Bucket>(
        "SELECT category AS bucket, COALESCE(SUM(cost_cents), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id = ? GROUP BY category ORDER BY cost_cents DESC",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;

    let (min_counter, max_counter, total_cost): (Option<i64>, Option<i64>, i64) = sqlx::query_as(
        "SELECT MIN(counter_value), MAX(counter_value), COALESCE(SUM(cost_cents), 0) \
         FROM activities WHERE object_id = ?",
    )
    .bind(object_id)
    .fetch_one(&state.db)
    .await?;

    let counter_span = min_counter.zip(max_counter).map(|(from, to)| Span { from, to });
    let span = counter_span.as_ref().map(|s| s.to - s.from).unwrap_or(0);
    let overall_cost_per_counter_milli = cost_per_counter_milli(total_cost, span);

    let fill_rows: Vec<(i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT counter_value, quantity_milli, cost_cents FROM activities \
         WHERE object_id = ? AND category = 'fuel' AND counter_value IS NOT NULL \
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

    Ok(Json(InsightsOut {
        by_year,
        by_category,
        counter_span,
        cost_per_counter_milli: overall_cost_per_counter_milli,
        fuel,
    }))
}
