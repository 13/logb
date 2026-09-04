use crate::auth::{AdminUser, AuthUser};
use crate::error::AppError;
use crate::state::App;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<App> {
    Router::new().route("/settings", get(read).put(write))
}

#[derive(Serialize, Deserialize)]
pub struct Settings {
    pub currency: String,
}

pub async fn currency(state: &App) -> Result<String, AppError> {
    let (v,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'currency'")
        .fetch_one(&state.db).await?;
    Ok(v)
}

async fn read(_: AuthUser, State(state): State<App>) -> Result<Json<Settings>, AppError> {
    Ok(Json(Settings { currency: currency(&state).await? }))
}

async fn write(AdminUser(_): AdminUser, State(state): State<App>, Json(body): Json<Settings>) -> Result<Json<Settings>, AppError> {
    let c = body.currency.trim();
    if c.len() != 3 || !c.chars().all(|ch| ch.is_ascii_uppercase()) {
        return Err(AppError::BadRequest("currency must be a 3-letter ISO code like EUR".into()));
    }
    sqlx::query("UPDATE settings SET value = ? WHERE key = 'currency'").bind(c).execute(&state.db).await?;
    Ok(Json(Settings { currency: c.to_string() }))
}
