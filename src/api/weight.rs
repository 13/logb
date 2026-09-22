//! Lightweight measurement history: no attachment joins or cumulative-counter calculations.
use super::objects::load_owned_object;
use crate::{auth::AuthUser, error::AppError, state::App};
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects/{id}/weight", get(history))
        .route("/objects/{id}/weight/summary", get(summary))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct WeightPoint {
    id: i64,
    date: String,
    created_at: String,
    weight_grams: i64,
}

#[derive(Serialize)]
pub struct WeightSummary {
    pub count: i64,
    pub latest_grams: Option<i64>,
    pub previous_grams: Option<i64>,
    pub minimum_grams: Option<i64>,
    pub maximum_grams: Option<i64>,
    pub average_grams: Option<i64>,
}

async fn history(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<WeightPoint>>, AppError> {
    load_owned_object(&state, user.id, id).await?;
    let rows = sqlx::query_as(
        "SELECT id, date, created_at, weight_grams FROM activities WHERE object_id = $1 \
         AND deleted_at IS NULL AND weight_grams IS NOT NULL AND date <= $2 \
         ORDER BY date DESC, created_at DESC, id DESC",
    )
    .bind(id)
    .bind(super::reminders::reading_horizon(user.today()))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn summary(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<Json<WeightSummary>, AppError> {
    load_owned_object(&state, user.id, id).await?;
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT weight_grams FROM activities WHERE object_id = $1 AND deleted_at IS NULL AND weight_grams IS NOT NULL ORDER BY date DESC, created_at DESC, id DESC")
        .bind(id).fetch_all(&state.db).await?;
    let values: Vec<i64> = rows.into_iter().map(|(v,)| v).collect();
    let count = values.len() as i64;
    Ok(Json(WeightSummary {
        count,
        latest_grams: values.first().copied(),
        previous_grams: values.get(1).copied(),
        minimum_grams: values.iter().copied().min(),
        maximum_grams: values.iter().copied().max(),
        average_grams: (!values.is_empty()).then(|| values.iter().sum::<i64>() / count),
    }))
}
