pub mod auth;
pub mod settings;
pub mod users;

use crate::state::App;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

pub fn router() -> Router<App> {
    Router::new()
        .route("/health", get(health))
        .merge(auth::router())
        .merge(users::router())
        .merge(settings::router())
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}
