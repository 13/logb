use crate::auth::AuthUser;
use crate::domain::stats::{purchase_spend, summarize, ObjectRow, Spend, Stats};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub fn router() -> Router<App> {
    Router::new().route("/stats", get(read)).route("/stats/energy", get(energy_usage))
}

#[derive(Serialize)]
pub struct EnergyUsageMonth {
    pub month: String,
    pub kwh_milli: i64,
    pub charges: i64,
    pub objects: i64,
}

#[derive(Serialize)]
pub struct EnergyUsage {
    pub months: Vec<EnergyUsageMonth>,
    pub current_kwh_milli: i64,
    pub previous_kwh_milli: i64,
}

#[derive(Deserialize)]
pub struct EnergyQuery {
    #[serde(default)]
    pub months: Option<u32>,
}

fn month_before(year: i32, month: u32, offset: u32) -> (i32, u32) {
    let absolute = year * 12 + month as i32 - 1 - offset as i32;
    (absolute.div_euclid(12), (absolute.rem_euclid(12) + 1) as u32)
}

async fn energy_usage(
    user: AuthUser,
    State(state): State<App>,
    Query(q): Query<EnergyQuery>,
) -> Result<Json<EnergyUsage>, AppError> {
    let count = q.months.unwrap_or(12).clamp(1, 36);
    let today = crate::db::today();
    let (year, month) = today
        .split_once('-')
        .and_then(|(y, rest)| rest.split_once('-').map(|(m, _)| (y, m)))
        .and_then(|(y, m)| Some((y.parse::<i32>().ok()?, m.parse::<u32>().ok()?)))
        .ok_or_else(|| AppError::Internal("server date is malformed".into()))?;
    let (start_year, start_month) = month_before(year, month, count - 1);
    let start = format!("{start_year:04}-{start_month:02}-01");
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT substr(a.date, 1, 7), CAST(SUM(a.quantity_milli) AS BIGINT), COUNT(*), COUNT(DISTINCT a.object_id) \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND o.deleted_at IS NULL AND a.deleted_at IS NULL \
           AND o.fuel_unit = 'kwh' AND a.category = 'fuel' AND a.quantity_milli IS NOT NULL \
           AND a.date >= $2 AND a.date <= $3 \
         GROUP BY substr(a.date, 1, 7)",
    )
    .bind(user.id)
    .bind(&start)
    .bind(&today)
    .fetch_all(&state.db)
    .await?;
    let by_month: std::collections::HashMap<String, (i64, i64, i64)> = rows
        .into_iter()
        .map(|(month, kwh, charges, objects)| (month, (kwh, charges, objects)))
        .collect();
    let mut months = Vec::with_capacity(count as usize);
    for offset in (0..count).rev() {
        let (y, m) = month_before(year, month, offset);
        let key = format!("{y:04}-{m:02}");
        let (kwh_milli, charges, objects) = by_month.get(&key).copied().unwrap_or_default();
        months.push(EnergyUsageMonth { month: key, kwh_milli, charges, objects });
    }
    let current_kwh_milli = months.last().map(|m| m.kwh_milli).unwrap_or(0);
    let previous_kwh_milli = months.iter().rev().nth(1).map(|m| m.kwh_milli).unwrap_or(0);
    Ok(Json(EnergyUsage { months, current_kwh_milli, previous_kwh_milli }))
}

#[derive(Deserialize)]
pub struct StatsQuery {
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub purchases: Option<bool>,
}

/// Spend across every object the user owns. The database groups; `domain::stats` does the rest,
/// because a household has tens of objects and the tree roll-up is plainer in Rust than in SQL
/// that has to run on two databases.
async fn read(user: AuthUser, State(state): State<App>, Query(q): Query<StatsQuery>) -> Result<Json<Stats>, AppError> {
    if let Some(y) = q.year {
        if !(0..=9999).contains(&y) {
            return Err(AppError::BadRequest("year must be between 0 and 9999".into()));
        }
    }

    // Readings are excluded as in insights: they never carry a cost.
    let rows: Vec<(i64, String, String, i64)> = sqlx::query_as(
        "SELECT a.object_id, substr(a.date, 1, 7), a.category, CAST(SUM(a.cost_cents) AS BIGINT) \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
           AND a.cost_cents IS NOT NULL AND a.category <> 'reading' \
         GROUP BY a.object_id, substr(a.date, 1, 7), a.category",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    let mut spend: Vec<Spend> = rows
        .into_iter()
        .map(|(object_id, month, category, cost_cents)| Spend { object_id, month, category, cost_cents })
        .collect();

    #[allow(clippy::type_complexity)]
    let object_rows: Vec<(i64, Option<i64>, String, String, Option<String>, Option<String>, Option<i64>, String)> = sqlx::query_as(
        "SELECT id, parent_id, name, type, archived_at, purchase_date, purchase_price_cents, created_at \
         FROM objects WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    let objects: Vec<ObjectRow> = object_rows
        .into_iter()
        .map(|(id, parent_id, name, kind, archived_at, purchase_date, purchase_price_cents, created_at)| ObjectRow {
            id, parent_id, name, kind, archived: archived_at.is_some(), purchase_date, purchase_price_cents, created_at,
        })
        .collect();

    if q.purchases.unwrap_or(false) {
        // A purchase entry with a cost is the purchase; see `purchase_spend`.
        let purchased: Vec<(i64,)> = sqlx::query_as(
            "SELECT DISTINCT a.object_id FROM activities a JOIN objects o ON o.id = a.object_id \
             WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
               AND a.category = 'purchase' AND a.cost_cents > 0",
        )
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
        let purchased: HashSet<i64> = purchased.into_iter().map(|(id,)| id).collect();
        spend.extend(purchase_spend(&objects, &purchased));
    }

    Ok(Json(summarize(&objects, &spend, q.year)))
}
