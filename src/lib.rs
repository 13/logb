pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod files;
pub mod spa;
pub mod state;

use axum::Router;
use config::Config;
use state::{App, AppState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

/// Build the application router with all state initialised (database created and migrated).
pub async fn build(config: Config) -> Result<Router, db::BoxError> {
    let db = db::connect(&config.data_dir).await?;
    let storage = files::Storage::new(&config.data_dir)?;
    let max_upload = config.max_upload_bytes();
    let max_import = config.max_import_bytes();
    let state: App = Arc::new(AppState {
        db,
        storage,
        config,
        login_attempts: Mutex::new(HashMap::new()),
    });
    Ok(Router::new()
        .nest("/api", api::router(max_upload, max_import))
        .fallback(spa::handler)
        // The default predicate already skips images, gRPC and event streams. Export archives
        // are the other already-compressed response memto serves: gzipping a zip burns CPU on
        // both ends for no gain.
        .layer(CompressionLayer::new().compress_when(
            DefaultPredicate::new().and(NotForContentType::const_new("application/zip")),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state))
}
