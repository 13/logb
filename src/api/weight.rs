//! Lightweight measurement history: no attachment joins or cumulative-counter calculations.
use axum::{extract::{Path, State}, routing::get, Json, Router};
use serde::Serialize;
use crate::{auth::AuthUser, error::AppError, state::App};
use super::objects::load_owned_object;

pub fn router() -> Router<App> {
    Router::new().route("/objects/{id}/weight", get(history))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct WeightPoint {
    id: i64,
    date: String,
    created_at: String,
    weight_grams: i64,
}

async fn history(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Json<Vec<WeightPoint>>, AppError> {
    load_owned_object(&state, user.id, id).await?;
    let rows = sqlx::query_as(
        "SELECT id, date, created_at, weight_grams FROM activities WHERE object_id = $1 \
         AND deleted_at IS NULL AND weight_grams IS NOT NULL AND date <= $2 \
         ORDER BY date DESC, created_at DESC, id DESC")
        .bind(id).bind(super::reminders::reading_horizon()).fetch_all(&state.db).await?;
    Ok(Json(rows))
}
