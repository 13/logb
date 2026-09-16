use super::objects::load_owned_object;
use crate::auth::AuthUser;
use crate::domain::energy::{energy, Charge, Energy, Trip};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

pub fn router() -> Router<App> {
    Router::new().route("/objects/{id}/energy", get(read))
}

#[derive(Serialize)]
pub struct EnergyOut {
    /// The object's `fuel_unit`, unchanged -- `null` on an object with no fuel unit at all.
    pub unit: Option<String>,
    pub price_milli: Option<i64>,
    #[serde(flatten)]
    pub figures: Energy,
}

/// Distance and cost per charge, and when to charge next -- see `domain::energy::energy` for
/// the maths. This handler only loads the rows it needs and maps them into the domain's own
/// types; no aggregation happens in SQL.
async fn read(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>) -> Result<Json<EnergyOut>, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;

    // Oldest first, as the brief asks; `energy` re-sorts by date itself for windowing, so this
    // order only matters for readability of a query someone might run by hand.
    #[allow(clippy::type_complexity)]
    let charge_rows: Vec<(String, i64, Option<i64>, Option<i64>, i64)> = sqlx::query_as(
        "SELECT date, counter_value, quantity_milli, cost_cents, charged_full FROM activities \
         WHERE object_id = $1 AND category = 'fuel' AND deleted_at IS NULL AND counter_value IS NOT NULL \
         ORDER BY date ASC, id ASC",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;
    let charges: Vec<Charge> = charge_rows
        .into_iter()
        .map(|(date, counter, quantity_milli, cost_cents, charged_full)| Charge {
            date,
            counter,
            quantity_milli,
            cost_cents,
            full: charged_full != 0,
        })
        .collect();

    // `start_counter`/`counter_value` are required on any stored trip (`ActivityInput::validate`),
    // so the `IS NOT NULL` guards are belt and braces, not a real filter -- the same reasoning
    // as `api::trips::trips_summary`'s own query.
    let trip_rows: Vec<(String, i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT date, start_counter, counter_value, battery_used_pct FROM activities \
         WHERE object_id = $1 AND category = 'trip' AND deleted_at IS NULL \
         AND start_counter IS NOT NULL AND counter_value IS NOT NULL",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;
    let trips: Vec<Trip> = trip_rows
        .into_iter()
        .map(|(date, start, end, battery_used_pct)| Trip {
            date,
            start_counter: start,
            distance: end - start,
            battery_used_pct,
        })
        .collect();

    let figures = energy(&charges, &trips, object.energy_price_milli);
    Ok(Json(EnergyOut { unit: object.fuel_unit, price_milli: object.energy_price_milli, figures }))
}
