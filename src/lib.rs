pub mod api;
pub mod auth;
pub mod backup;
pub mod config;
pub mod copy;
pub mod db;
pub mod dialect;
pub mod domain;
pub mod error;
pub mod files;
pub mod notify;
pub mod object_type;
pub mod pointer;
pub mod restore;
pub mod spa;
pub mod state;
pub mod sync;
pub mod tasks;

use axum::Router;
use config::Config;
use state::{App, AppState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use axum::http::{header, HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use rand::RngExt;
use tracing::Instrument;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::compression::CompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

/// Baseline response headers for everything LogB serves.
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

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Tags every request with an id, puts it on the tracing span the handler runs in, and echoes
/// it back on the response.
///
/// Without it a 500 in the log -- which deliberately says only "internal error" to the client
/// -- cannot be tied to the request that produced it. An id supplied by a front proxy is kept
/// so the two logs line up.
async fn request_id(mut req: Request<axum::body::Body>, next: Next) -> Response {
    let id = req
        .headers()
        .get(&REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty() && v.len() <= 64 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let mut bytes = [0u8; 8];
            rand::rng().fill(&mut bytes);
            hex::encode(bytes)
        });
    if let Ok(value) = HeaderValue::from_str(&id) {
        req.headers_mut().insert(&REQUEST_ID, value.clone());
        let span = tracing::info_span!("request", request_id = %id);
        let mut res = next.run(req).instrument(span).await;
        res.headers_mut().insert(&REQUEST_ID, value);
        return res;
    }
    next.run(req).await
}

/// Build the application router with all state initialised (database created and migrated).
pub async fn build(config: Config) -> Result<Router, db::BoxError> {
    Ok(build_with_state(config).await?.0)
}

/// As `build`, but also hands back the shared state, for callers that run background work
/// against it (the reminder digest scheduler). Tests use `build`, so they never start it.
pub async fn build_with_state(config: Config) -> Result<(Router, App), db::BoxError> {
    db::set_timezone(config.timezone);
    let url = config.database_url()?;
    let backend = dialect::Backend::of(&url);
    // PostgreSQL is not a supported configuration yet: the README's LOGB_DATABASE_URL row names
    // the same gap. This is the line that is already in an operator's scrollback when it bites,
    // rather than something they had to have read in advance. `logb --copy-to` (part three) now
    // gets an existing SQLite database across, so that half of the old warning is gone -- but
    // parts four and five (switching over, and PostgreSQL's own backup story) are still unbuilt,
    // so this stays a warning rather than becoming an endorsement.
    if backend != dialect::Backend::Sqlite {
        tracing::warn!(
            "LOGB_DATABASE_URL points at PostgreSQL, which is not a supported configuration yet: \
             LogB takes no automatic backups there -- backing it up is your own job, with \
             PostgreSQL's own tooling"
        );
    }
    let db = db::connect_with_pool_size(&url, config.db_pool_size).await?;
    let storage = files::Storage::new(&config.data_dir)?;
    let max_upload = config.max_upload_bytes();
    let max_import = config.max_import_bytes();
    let state: App = Arc::new(AppState {
        db,
        backend,
        storage,
        config,
        login_attempts: Mutex::new(HashMap::new()),
    });
    // Off unless configured: the bundled SPA is same-origin and needs none of this. It exists
    // for a SEPARATE web client -- another origin in development, say -- which cannot call the
    // API at all without it.
    //
    // `allow_credentials` is deliberately absent. Permitting it would let a listed origin make
    // the browser attach the session cookie to requests that site initiated, which is the
    // classic cross-site request forgery shape. A cross-origin client uses a bearer token
    // instead: that travels only because the client put it there.
    let cors = state.config.cors_origin_list();
    let base = Router::new()
        .nest("/api", api::router(max_upload, max_import))
        .fallback(spa::handler)
        // The default predicate already skips images, gRPC and event streams. Export archives
        // are the other already-compressed response LogB serves: gzipping a zip burns CPU on
        // both ends for no gain.
        .layer(CompressionLayer::new().compress_when(
            DefaultPredicate::new().and(NotForContentType::const_new("application/zip")),
        ))
        .layer(TraceLayer::new_for_http());
    // Applied with an `if` rather than an always-present layer, so an instance that has not
    // configured any origin behaves exactly as it did before this existed.
    let base = if cors.is_empty() {
        base
    } else {
        base.layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(cors)
                .allow_methods(tower_http::cors::AllowMethods::mirror_request())
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]),
        )
    };
    let router = base
        .layer(axum::middleware::from_fn(request_id))
        .layer(SetResponseHeaderLayer::if_not_present(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP)))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .with_state(state.clone());
    Ok((router, state))
}
