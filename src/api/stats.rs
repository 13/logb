use crate::auth::AuthUser;
use crate::domain::stats::{purchase_spend, summarize, ObjectRow, Spend, Stats};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use std::collections::HashSet;

pub fn router() -> Router<App> {
    Router::new().route("/stats", get(read))
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
        if !(1900..=9999).contains(&y) {
            return Err(AppError::BadRequest("year must be between 1900 and 9999".into()));
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
