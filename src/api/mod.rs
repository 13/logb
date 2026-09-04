pub mod activities;
pub mod attachments;
pub mod auth;
pub mod objects;
pub mod reminders;
pub mod settings;
pub mod users;

use crate::state::App;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

pub fn router(max_upload_bytes: usize) -> Router<App> {
    Router::new()
        .route("/health", get(health))
        .merge(auth::router())
        .merge(users::router())
        .merge(settings::router())
        .merge(objects::router())
        .merge(activities::router())
        .merge(reminders::router())
        .merge(attachments::router(max_upload_bytes))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}
