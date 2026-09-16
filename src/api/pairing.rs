//! QR sign-in's two endpoints: a signed-in browser asks for a one-time code
//! (`POST /auth/pair`), a phone swaps it for an API token (`POST /auth/pair/redeem`). The code
//! itself -- generation, hashing, the URI and its QR rendering -- lives in `domain::pairing`;
//! this module owns the database work and the HTTP shapes around it.

use crate::auth::{self, SessionUser};
use crate::db;
use crate::domain::pairing;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;

pub fn router() -> Router<App> {
    Router::new()
        .route("/auth/pair", post(create_pair))
        .route("/auth/pair/redeem", post(redeem))
}

/// The origin this instance is reachable at, to embed in a pairing URI's `server` parameter.
///
/// `LOGB_PUBLIC_URL` (`state.config.public_url`) is authoritative when set: it already exists
/// for exactly this question -- "where do I tell someone this instance lives" -- and `notify`
/// uses it the same way to put links into the reminder digest. Reusing it here means an
/// operator only states their public address once.
///
/// Unset, the origin is rebuilt from the request the browser making this call actually used:
/// the scheme from `auth::wants_secure` -- the same signal that decides the session cookie's
/// `Secure` flag -- and the host from the `Host` header. `X-Forwarded-Host` is read instead
/// only when `LOGB_TRUST_PROXY` says this deployment sits behind a proxy that overwrites it --
/// the same trust boundary `auth::client_ip` already applies to `X-Forwarded-For` -- so a
/// caller cannot hand a signed-in browser's own QR code an arbitrary host by forging a header
/// on a deployment that never promised to rewrite one.
fn public_base_url(state: &App, headers: &HeaderMap) -> String {
    if let Some(url) = state.config.public_url.as_deref() {
        let trimmed = url.trim_end_matches('/');
        return format!("{trimmed}/");
    }
    let scheme = if auth::wants_secure(state, headers) { "https" } else { "http" };
    let forwarded_host = state
        .config
        .trust_proxy
        .then(|| headers.get("x-forwarded-host"))
        .flatten()
        .and_then(|v| v.to_str().ok());
    let host = forwarded_host
        .or_else(|| headers.get(axum::http::header::HOST).and_then(|v| v.to_str().ok()))
        .unwrap_or("localhost");
    format!("{scheme}://{host}/")
}

/// Issues a fresh pairing code for the caller: a browser's Account page, not the phone.
///
/// `SessionUser`, exactly like `create_token` -- creating something that can sign a device in
/// needs an interactive login, never a token that could mint another credential on its own.
async fn create_pair(
    SessionUser(user): SessionUser,
    State(state): State<App>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let now = db::now();
    let code = pairing::new_code();
    let expires_at = (chrono::Utc::now() + chrono::Duration::from_std(pairing::TTL).unwrap())
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // A new code replaces whatever this user already had -- used, expired, or still perfectly
    // live -- not just the dead rows: the frontend shows only ever the most recent code, so an
    // older one that could still be redeemed would be a working sign-in nobody can see any more
    // to notice or revoke. Delete and insert share one write transaction (the same
    // `db::begin_write` `redeem` uses) so a crash between them cannot leave this user with the
    // old code deleted and no new one in its place.
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("DELETE FROM pairing_codes WHERE user_id = $1")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO pairing_codes (user_id, code_hash, created_at, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(user.id)
        .bind(pairing::hash(&code))
        .bind(&now)
        .bind(&expires_at)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let base_url = public_base_url(&state, &headers);
    let uri = pairing::uri(&base_url, &code);
    let qr_svg = pairing::qr_svg(&uri);
    Ok((StatusCode::CREATED, Json(json!({
        "code": code,
        "uri": uri,
        "qr_svg": qr_svg,
        "expires_at": expires_at,
    }))))
}

#[derive(Deserialize)]
struct PairRedeem {
    code: String,
    device_name: String,
}

/// `LogB Android · <device name>`, trimmed and capped at 64 characters -- the same length
/// `create_token` enforces on a name a person types, applied here to one a device supplies
/// instead.
fn device_token_name(device_name: &str) -> String {
    let full = format!("LogB Android · {}", device_name.trim());
    full.chars().take(64).collect()
}

/// Swaps a one-time code for a named API token. No session or token of its own is needed -- the
/// code itself is the credential -- so this is rate-limited exactly like `login`, by the
/// calling IP.
///
/// Unknown, expired and already-used codes are told apart from nothing: the `UPDATE ...
/// RETURNING` below only ever matches a code that is both unused and unexpired, so every other
/// case reaches the same `AppError::Unauthorized` and therefore the same response body. A
/// caller probing codes learns nothing about which of the three it tried.
async fn redeem(
    State(state): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PairRedeem>,
) -> Result<Json<serde_json::Value>, AppError> {
    auth::check_login_rate(&state, auth::client_ip(&state, &headers, peer))?;

    let now = db::now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let redeemed: Option<(i64,)> = sqlx::query_as(
        "UPDATE pairing_codes SET used_at = $1 WHERE code_hash = $2 AND used_at IS NULL AND expires_at > $3 \
         RETURNING user_id",
    )
    .bind(&now)
    .bind(pairing::hash(&body.code))
    .bind(&now)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_id,)) = redeemed else {
        return Err(AppError::Unauthorized);
    };

    // Minted in the same transaction that marked the code used: a crash between the two would
    // otherwise either burn a valid code for no token, or hand out a token from a redeem that
    // never committed.
    let token = auth::new_api_token();
    let name = device_token_name(&body.device_name);
    let id: (i64,) = sqlx::query_as(
        "INSERT INTO api_tokens (user_id, name, token_hash, prefix, created_at) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(user_id)
    .bind(&name)
    .bind(auth::hash_api_token(&token))
    .bind(auth::token_prefix(&token))
    .bind(&now)
    .fetch_one(&mut *tx)
    .await?;
    let user: (i64, String) = sqlx::query_as("SELECT id, username FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(json!({
        "token": token,
        "token_id": id.0,
        "user": { "id": user.0, "username": user.1 },
    })))
}
