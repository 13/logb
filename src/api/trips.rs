use super::objects::{load_owned_object, validate_date};
use crate::auth::AuthUser;
use crate::db;
use crate::domain::trips::{self, TripRow, TripTotals};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects/{id}/trip-places", get(trip_places))
        .route("/objects/{id}/trips/summary", get(trips_summary))
}

#[derive(Serialize)]
pub struct TripPlaces {
    pub from: Vec<String>,
    pub to: Vec<String>,
}

/// How many distinct places each of `from`/`to` carries at most -- the same cap as
/// `activities::recent_titles`' suggestion list.
const PLACE_LIMIT: usize = 20;

/// `places`, most recent trip first (the order the caller's query already reads them in),
/// deduplicated and capped -- the first occurrence of a place is its most recent one.
fn distinct_places(places: impl Iterator<Item = Option<String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for place in places.flatten() {
        if out.len() >= PLACE_LIMIT {
            break;
        }
        if seen.insert(place.clone()) {
            out.push(place);
        }
    }
    out
}

/// Earlier `from`/`to` places of this object's trips, for the entry form's suggestions -- see
/// the spec's "From / To suggest earlier places of this object".
async fn trip_places(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>) -> Result<Json<TripPlaces>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let rows: Vec<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT from_place, to_place FROM activities \
         WHERE object_id = $1 AND category = 'trip' AND deleted_at IS NULL \
         ORDER BY date DESC, id DESC",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;
    let from = distinct_places(rows.iter().map(|(f, _)| f.clone()));
    let to = distinct_places(rows.iter().map(|(_, t)| t.clone()));
    Ok(Json(TripPlaces { from, to }))
}

#[derive(Deserialize)]
pub struct SummaryQuery {
    /// `YYYY-MM-DD`; defaults to the server's own date in the configured timezone
    /// (`db::today`, the same helper `api::insights` uses).
    #[serde(default)]
    pub today: Option<String>,
}

#[derive(Serialize)]
pub struct TripSummary {
    pub month: TripTotals,
    pub year: TripTotals,
    pub all: TripTotals,
}

/// Trip counts and distance for this month, this year and all time -- see
/// `domain::trips::totals` for the aggregation, and the spec's "Totals" section.
async fn trips_summary(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<TripSummary>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let today = match q.today {
        Some(t) => {
            validate_date(&t)?;
            t
        }
        None => db::today(),
    };
    // `start_counter`/`counter_value` are required on any stored trip (`ActivityInput::validate`),
    // so the `IS NOT NULL` guards are belt and braces, not a real filter.
    #[allow(clippy::type_complexity)]
    let rows: Vec<(String, i64, i64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT date, start_counter, counter_value, duration_minutes, battery_used_pct \
         FROM activities WHERE object_id = $1 AND category = 'trip' AND deleted_at IS NULL \
         AND start_counter IS NOT NULL AND counter_value IS NOT NULL",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;
    let trip_rows: Vec<TripRow> = rows
        .into_iter()
        .map(|(date, start, end, duration_minutes, battery_used_pct)| TripRow { date, start, end, duration_minutes, battery_used_pct })
        .collect();

    let year_prefix = &today[..4];
    let month_prefix = &today[..7];
    Ok(Json(TripSummary {
        month: trips::totals(&trip_rows, month_prefix),
        year: trips::totals(&trip_rows, year_prefix),
        all: trips::totals(&trip_rows, ""),
    }))
}
