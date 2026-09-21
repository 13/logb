use crate::auth::AuthUser;
use crate::domain::stats::{purchase_spend, summarize, ObjectRow, Spend, Stats};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

type WaterObjectAccumulator = (String, String, Option<i64>, i64, Option<i64>);
type FuelLevelRow = (i64, String, String, Option<i64>, Option<i64>, String, i64);

pub fn router() -> Router<App> {
    Router::new()
        .route("/stats", get(read))
        .route("/stats/energy", get(energy_usage))
        .route("/stats/fuel", get(fuel_usage))
        .route("/stats/water", get(water_usage))
}

#[derive(Serialize)]
pub struct WaterUsageMonth {
    pub month: String,
    /// Canonical volume. 1000 is one litre, regardless of the meter's display unit.
    pub liters_milli: i64,
    pub cost_cents: i64,
    pub entries: i64,
    pub estimated: bool,
}

#[derive(Serialize)]
pub struct WaterObjectUsage {
    pub object_id: i64,
    pub object_name: String,
    pub liters_milli: i64,
    pub target_liters_milli: Option<i64>,
    pub unit: String,
    pub current_reading_milli: Option<i64>,
}

#[derive(Serialize)]
pub struct WaterUsage {
    pub months: Vec<WaterUsageMonth>,
    pub current_liters_milli: i64,
    pub previous_liters_milli: i64,
    pub current_cost_cents: i64,
    pub daily_average_liters_milli: i64,
    pub anomalies: i64,
    pub objects: Vec<WaterObjectUsage>,
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
    pub target_kwh_milli: i64,
}

#[derive(Serialize)]
pub struct FuelUsageMonth {
    pub month: String,
    pub liters_milli: i64,
    pub gallons_milli: i64,
    pub charges: i64,
    pub objects: i64,
}

#[derive(Serialize)]
pub struct FuelUsage {
    pub months: Vec<FuelUsageMonth>,
    pub current_liters_milli: i64,
    pub previous_liters_milli: i64,
    pub current_gallons_milli: i64,
    pub previous_gallons_milli: i64,
    pub levels: Vec<FuelTankLevel>,
}

#[derive(Serialize)]
pub struct FuelTankLevel {
    pub object_id: i64,
    pub object_name: String,
    pub unit: String,
    pub date: String,
    pub level_pct: i64,
    pub capacity_milli: Option<i64>,
    pub remaining_milli: Option<i64>,
    pub low: bool,
    pub estimated_days_remaining: Option<i64>,
}

#[derive(Deserialize)]
pub struct EnergyQuery {
    #[serde(default)]
    pub months: Option<u32>,
}

fn month_before(year: i32, month: u32, offset: u32) -> (i32, u32) {
    let absolute = year * 12 + month as i32 - 1 - offset as i32;
    (
        absolute.div_euclid(12),
        (absolute.rem_euclid(12) + 1) as u32,
    )
}

fn as_liters_milli(value_milli: i64, unit: &str) -> i64 {
    match unit {
        "m3" => value_milli.saturating_mul(1000),
        "gal" => ((value_milli as i128 * 3_785_412) / 1_000_000)
            .clamp(i64::MIN as i128, i64::MAX as i128) as i64,
        _ => value_milli,
    }
}

async fn water_usage(
    user: AuthUser,
    State(state): State<App>,
    Query(q): Query<EnergyQuery>,
) -> Result<Json<WaterUsage>, AppError> {
    let count = q.months.unwrap_or(12).clamp(1, 36);
    let today = crate::db::today();
    let (year, month) = today
        .split_once('-')
        .and_then(|(y, rest)| rest.split_once('-').map(|(m, _)| (y, m)))
        .and_then(|(y, m)| Some((y.parse::<i32>().ok()?, m.parse::<u32>().ok()?)))
        .ok_or_else(|| AppError::Internal("server date is malformed".into()))?;
    let (start_year, start_month) = month_before(year, month, count - 1);
    let start_month_key = format!("{start_year:04}-{start_month:02}");
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, String, Option<i64>, String, Option<i64>, Option<i64>, Option<i64>, i64, i64)> = sqlx::query_as(
        "SELECT o.id, o.name, o.resource_unit, o.monthly_target_milli, a.date, a.quantity_milli, a.meter_reading_milli, a.cost_cents, a.estimated, a.meter_reset \
         FROM objects o JOIN activities a ON a.object_id = o.id \
         WHERE o.user_id = $1 AND o.deleted_at IS NULL AND a.deleted_at IS NULL AND o.resource_kind = 'water' \
           AND a.category = 'usage' AND a.date <= $2 ORDER BY o.id, a.date, a.id",
    ).bind(user.id).bind(&today).fetch_all(&state.db).await?;

    use std::collections::HashMap;
    let mut by_month: HashMap<String, (i64, i64, i64, bool)> = HashMap::new();
    let mut by_object: HashMap<i64, WaterObjectAccumulator> = HashMap::new();
    let mut previous_reading: HashMap<i64, i64> = HashMap::new();
    let mut anomalies = 0;
    for (object_id, name, unit, target, date, quantity, reading, cost, estimated, reset) in rows {
        let key = date.get(..7).unwrap_or("").to_string();
        let object = by_object
            .entry(object_id)
            .or_insert((name, unit.clone(), target, 0, None));
        if let Some(r) = reading {
            object.4 = Some(r);
        }
        let amount = if let Some(qty) = quantity {
            Some(as_liters_milli(qty, &unit))
        } else if let Some(current) = reading {
            let delta = previous_reading
                .insert(object_id, current)
                .and_then(|prev| {
                    if reset == 1 {
                        None
                    } else if current >= prev {
                        Some(current - prev)
                    } else {
                        anomalies += 1;
                        None
                    }
                });
            delta.map(|v| as_liters_milli(v, &unit))
        } else {
            None
        };
        if key < start_month_key {
            continue;
        }
        if let Some(amount) = amount {
            let month = by_month.entry(key.clone()).or_default();
            month.0 = month.0.saturating_add(amount);
            month.2 += 1;
            month.3 |= estimated == 1;
            object.3 = object.3.saturating_add(amount);
        }
        if let Some(cost) = cost {
            by_month.entry(key).or_default().1 += cost;
        }
    }
    let mut months = Vec::with_capacity(count as usize);
    for offset in (0..count).rev() {
        let (y, m) = month_before(year, month, offset);
        let key = format!("{y:04}-{m:02}");
        let (liters_milli, cost_cents, entries, estimated) =
            by_month.get(&key).copied().unwrap_or_default();
        months.push(WaterUsageMonth {
            month: key,
            liters_milli,
            cost_cents,
            entries,
            estimated,
        });
    }
    let current = months
        .last()
        .map(|m| (m.liters_milli, m.cost_cents))
        .unwrap_or_default();
    let previous = months
        .iter()
        .rev()
        .nth(1)
        .map(|m| m.liters_milli)
        .unwrap_or(0);
    let day = today
        .get(8..10)
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(1)
        .max(1);
    let mut objects: Vec<_> = by_object
        .into_iter()
        .map(
            |(object_id, (object_name, unit, target, liters_milli, current_reading_milli))| {
                WaterObjectUsage {
                    object_id,
                    object_name,
                    liters_milli,
                    target_liters_milli: target.map(|v| as_liters_milli(v, &unit)),
                    unit,
                    current_reading_milli,
                }
            },
        )
        .collect();
    objects.sort_by(|a, b| {
        a.object_name
            .to_lowercase()
            .cmp(&b.object_name.to_lowercase())
    });
    Ok(Json(WaterUsage {
        months,
        current_liters_milli: current.0,
        previous_liters_milli: previous,
        current_cost_cents: current.1,
        daily_average_liters_milli: current.0 / day,
        anomalies,
        objects,
    }))
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
           AND COALESCE(o.resource_unit, o.fuel_unit) = 'kwh' AND (o.resource_kind = 'electricity' OR o.resource_kind IS NULL) \
           AND a.category IN ('fuel','usage') AND a.quantity_milli IS NOT NULL \
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
        months.push(EnergyUsageMonth {
            month: key,
            kwh_milli,
            charges,
            objects,
        });
    }
    let current_kwh_milli = months.last().map(|m| m.kwh_milli).unwrap_or(0);
    let previous_kwh_milli = months.iter().rev().nth(1).map(|m| m.kwh_milli).unwrap_or(0);
    // PostgreSQL widens SUM(BIGINT) to NUMERIC while SQLite keeps an integer. Cast the final
    // value, as the grouped query above already does, so both backends decode into i64.
    let target_kwh_milli: i64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(SUM(monthly_target_milli), 0) AS BIGINT) FROM objects WHERE user_id = $1 AND deleted_at IS NULL AND resource_kind = 'electricity'")
        .bind(user.id).fetch_one(&state.db).await?;
    Ok(Json(EnergyUsage {
        months,
        current_kwh_milli,
        previous_kwh_milli,
        target_kwh_milli,
    }))
}

async fn fuel_usage(
    user: AuthUser,
    State(state): State<App>,
    Query(q): Query<EnergyQuery>,
) -> Result<Json<FuelUsage>, AppError> {
    let count = q.months.unwrap_or(12).clamp(1, 36);
    let today = crate::db::today();
    let (year, month) = today
        .split_once('-')
        .and_then(|(y, rest)| rest.split_once('-').map(|(m, _)| (y, m)))
        .and_then(|(y, m)| Some((y.parse::<i32>().ok()?, m.parse::<u32>().ok()?)))
        .ok_or_else(|| AppError::Internal("server date is malformed".into()))?;
    let (start_year, start_month) = month_before(year, month, count - 1);
    let start = format!("{start_year:04}-{start_month:02}-01");
    let rows: Vec<(String, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT substr(a.date, 1, 7), COALESCE(o.resource_unit, o.fuel_unit), CAST(SUM(a.quantity_milli) AS BIGINT), COUNT(*), COUNT(DISTINCT a.object_id) \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND o.deleted_at IS NULL AND a.deleted_at IS NULL \
           AND COALESCE(o.resource_unit, o.fuel_unit) IN ('l', 'gal') AND (o.resource_kind IS NULL OR o.resource_kind <> 'water') \
           AND a.category IN ('fuel','usage') AND a.quantity_milli IS NOT NULL \
           AND a.date >= $2 AND a.date <= $3 \
         GROUP BY substr(a.date, 1, 7), COALESCE(o.resource_unit, o.fuel_unit)",
    )
    .bind(user.id).bind(&start).bind(&today).fetch_all(&state.db).await?;
    let mut by_month: std::collections::HashMap<String, (i64, i64, i64, i64, i64)> =
        std::collections::HashMap::new();
    for (month_key, unit, quantity, charges, objects) in rows {
        let entry = by_month.entry(month_key).or_default();
        if unit == "l" {
            entry.0 += quantity;
            entry.2 += charges;
            entry.4 += objects;
        } else {
            entry.1 += quantity;
            entry.3 += charges;
            entry.4 += objects;
        }
    }
    let mut months = Vec::with_capacity(count as usize);
    for offset in (0..count).rev() {
        let (y, m) = month_before(year, month, offset);
        let key = format!("{y:04}-{m:02}");
        let (liters, gallons, l_charges, g_charges, objects) =
            by_month.get(&key).copied().unwrap_or_default();
        months.push(FuelUsageMonth {
            month: key,
            liters_milli: liters,
            gallons_milli: gallons,
            charges: l_charges + g_charges,
            objects,
        });
    }
    let current = months
        .last()
        .map(|m| (m.liters_milli, m.gallons_milli))
        .unwrap_or_default();
    let previous = months
        .iter()
        .rev()
        .nth(1)
        .map(|m| (m.liters_milli, m.gallons_milli))
        .unwrap_or_default();
    let level_rows: Vec<FuelLevelRow> = sqlx::query_as(
        "SELECT o.id, o.name, COALESCE(o.resource_unit, o.fuel_unit), o.fuel_capacity_milli, o.low_level_pct, a.date, a.fuel_level_pct \
         FROM objects o JOIN activities a ON a.object_id = o.id \
         WHERE o.user_id = $1 AND o.deleted_at IS NULL AND a.deleted_at IS NULL \
           AND COALESCE(o.resource_unit, o.fuel_unit) IN ('l', 'gal') AND (o.resource_kind IS NULL OR o.resource_kind <> 'water') \
           AND a.category IN ('fuel','usage') AND a.fuel_level_pct IS NOT NULL AND a.date <= $2 \
         ORDER BY a.date DESC, a.id DESC",
    )
    .bind(user.id).bind(&today).fetch_all(&state.db).await?;
    let since = (chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d")
        .map_err(|_| AppError::Internal("server date is malformed".into()))?
        - chrono::Days::new(90))
    .format("%Y-%m-%d")
    .to_string();
    let recent_rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT a.object_id, CAST(SUM(a.quantity_milli) AS BIGINT) FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND o.deleted_at IS NULL AND a.deleted_at IS NULL AND a.category IN ('fuel','usage') \
         AND a.quantity_milli IS NOT NULL AND a.date >= $2 AND a.date <= $3 GROUP BY a.object_id")
        .bind(user.id).bind(&since).bind(&today).fetch_all(&state.db).await?;
    let recent: std::collections::HashMap<i64, i64> = recent_rows.into_iter().collect();
    let mut seen = HashSet::new();
    let levels = level_rows
        .into_iter()
        .filter_map(
            |(object_id, object_name, unit, capacity_milli, low_level_pct, date, level_pct)| {
                if !seen.insert(object_id) {
                    return None;
                }
                let remaining_milli = capacity_milli.map(|capacity| capacity * level_pct / 100);
                let estimated_days_remaining = remaining_milli
                    .zip(recent.get(&object_id).copied())
                    .and_then(|(remaining, used)| {
                        (used > 0).then(|| remaining.saturating_mul(90) / used)
                    });
                Some(FuelTankLevel {
                    object_id,
                    object_name,
                    unit,
                    date,
                    level_pct,
                    capacity_milli,
                    remaining_milli,
                    low: low_level_pct.is_some_and(|threshold| level_pct <= threshold),
                    estimated_days_remaining,
                })
            },
        )
        .collect();
    Ok(Json(FuelUsage {
        months,
        current_liters_milli: current.0,
        previous_liters_milli: previous.0,
        current_gallons_milli: current.1,
        previous_gallons_milli: previous.1,
        levels,
    }))
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
async fn read(
    user: AuthUser,
    State(state): State<App>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<Stats>, AppError> {
    if let Some(y) = q.year {
        if !(0..=9999).contains(&y) {
            return Err(AppError::BadRequest(
                "year must be between 0 and 9999".into(),
            ));
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
        .map(|(object_id, month, category, cost_cents)| Spend {
            object_id,
            month,
            category,
            cost_cents,
        })
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
        .map(
            |(
                id,
                parent_id,
                name,
                kind,
                archived_at,
                purchase_date,
                purchase_price_cents,
                created_at,
            )| ObjectRow {
                id,
                parent_id,
                name,
                kind,
                archived: archived_at.is_some(),
                purchase_date,
                purchase_price_cents,
                created_at,
            },
        )
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
