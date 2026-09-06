pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod files;
pub mod notify;
pub mod spa;
pub mod state;
pub mod tasks;

use axum::Router;
use config::Config;
use state::{App, AppState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use axum::http::{header, HeaderValue};
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::compression::CompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

/// Baseline response headers for everything memto serves.
///
/// Every header is set only `if_not_present`, so a handler that needs something stricter --
/// the attachment routes, which serve user-supplied bytes under a sandbox policy -- keeps its
/// own value.
///
/// The page policy allows nothing off-origin: the SPA is a self-contained bundle with no CDN,
/// no analytics and no remote fonts. `style-src` keeps `'unsafe-inline'` because Svelte emits
/// inline `style` attributes; scripts have no such exemption.
const CSP: &str = "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; \
script-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; object-src 'none'; \
frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// Build the application router with all state initialised (database created and migrated).
pub async fn build(config: Config) -> Result<Router, db::BoxError> {
    Ok(build_with_state(config).await?.0)
}

/// As `build`, but also hands back the shared state, for callers that run background work
/// against it (the reminder digest scheduler). Tests use `build`, so they never start it.
pub async fn build_with_state(config: Config) -> Result<(Router, App), db::BoxError> {
    db::set_timezone(config.timezone);
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
    let router = Router::new()
        .nest("/api", api::router(max_upload, max_import))
        .fallback(spa::handler)
        // The default predicate already skips images, gRPC and event streams. Export archives
        // are the other already-compressed response memto serves: gzipping a zip burns CPU on
        // both ends for no gain.
        .layer(CompressionLayer::new().compress_when(
            DefaultPredicate::new().and(NotForContentType::const_new("application/zip")),
        ))
        .layer(TraceLayer::new_for_http())
        .layer(SetResponseHeaderLayer::if_not_present(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP)))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .with_state(state.clone());
    Ok((router, state))
}
