pub mod activities;
pub mod attachments;
pub mod auth;
pub mod database;
pub mod export;
pub mod insights;
pub mod objects;
pub mod reminders;
pub mod search;
pub mod settings;
pub mod sync;
pub mod users;

use crate::error::AppError;
use crate::state::App;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

pub fn router(max_upload_bytes: usize, max_import_bytes: usize) -> Router<App> {
    Router::new()
        .route("/health", get(health))
        .merge(auth::router())
        .merge(users::router())
        .merge(settings::router())
        .merge(database::router())
        .merge(objects::router())
        .merge(insights::router())
        .merge(activities::router())
        .merge(reminders::router())
        .merge(search::router())
        .merge(sync::router())
        .merge(attachments::router(max_upload_bytes))
        .merge(export::router(max_import_bytes))
}

/// Liveness that is worth the name: it answers for the database, not just the process.
///
/// A handler returning a constant cannot distinguish "serving correctly" from "serving an empty
/// file", and an instance in the second state once ran for nineteen hours reporting itself
/// healthy. Counting applied migrations catches both halves of that: a database with no schema,
/// and one whose schema is older than the binary talking to it.
///
/// The comparison is deliberately one-directional: `applied < expected`. An ahead schema (more
/// migrations applied than embedded in this binary) is tolerated and reports healthy. This is
/// correct for rollbacks: when a deployment rolls back to an older release, the database schema
/// is ahead of the binary, but all applied migrations are additive (`CREATE TABLE`, `CREATE
/// INDEX`, `ALTER TABLE ADD COLUMN`), so the older binary genuinely does work against the newer
/// schema. Failing health on an ahead schema would make rollbacks permanently unhealthy and
/// unrecoverable. This assumption is load-bearing: a future destructive migration (dropped or
/// renamed column, tightened `NOT NULL`) would break it, and this check would not notice. That
/// constraint must be maintained.
async fn health(State(state): State<App>) -> Result<Json<serde_json::Value>, AppError> {
    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "health check: database query failed");
            AppError::Unavailable("database unavailable".into())
        })?;
    // The count to compare against is the one for *this* database's backend: SQLite carries
    // nine migrations and PostgreSQL one, so counting the wrong set would report a healthy
    // PostgreSQL instance as eight migrations behind. A URL this instance could not resolve
    // is not something health can diagnose, so fall back to the applied count and let the
    // check pass rather than fail an otherwise working instance on a config error.
    let expected = state
        .config
        .database_url()
        .map(|url| crate::db::expected_migrations(&url) as i64)
        .unwrap_or(applied);
    if applied < expected {
        return Err(AppError::Unavailable(format!(
            "schema is behind: {applied} of {expected} migrations applied"
        )));
    }
    Ok(Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "migrations": applied,
    })))
}

/// A blank (after trimming) `client_op_id` means the same thing as an absent one: no
/// idempotency was requested for this create. Both the JSON (`activities::create`) and
/// multipart (`attachments::upload`) paths must funnel their input through this before it can
/// reach the partial unique index, because a client that always populates the field --
/// `FormData.append("client_op_id", id ?? "")`, or a JSON serializer that never omits an
/// optional -- would otherwise send the literal string `""` for two genuinely different
/// creates on the same object. Treating `""` as a real id would collide those two creates on
/// the index and resolve the second to the first row: a lost write, which is exactly the
/// failure this whole mechanism exists to prevent. A blank is naive, not hostile, so it is
/// normalised rather than rejected -- the write still lands, just without idempotency.
pub(crate) fn normalize_op_id(raw: Option<String>) -> Option<String> {
    raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The op id a client_op_id lookup resolved to is spoken for by a row this request cannot be
/// handed: one belonging to another object, or one that has since been deleted -- the id stays
/// taken (the unique index spans tombstones too), it just no longer names a row the caller can
/// be shown. Shared by `activities` and `attachments`, whose create paths hit the same case.
pub(crate) fn op_id_conflict() -> AppError {
    AppError::Conflict("client_op_id already used".into())
}
