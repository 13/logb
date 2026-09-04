pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod state;

use axum::Router;
use config::Config;
use state::{App, AppState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Build the application router with all state initialised (database created and migrated).
pub async fn build(config: Config) -> Result<Router, db::BoxError> {
    let db = db::connect(&config.data_dir).await?;
    let state: App = Arc::new(AppState {
        db,
        config,
        login_attempts: Mutex::new(HashMap::new()),
    });
    Ok(Router::new()
        .nest("/api", api::router())
        .with_state(state))
}
