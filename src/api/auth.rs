use crate::auth::{self, AuthUser, SessionUser};
use crate::db;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::extract::Path;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;

pub fn router() -> Router<App> {
    Router::new()
        .route("/auth/status", get(status))
        .route("/auth/setup", post(setup))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-all", post(logout_all))
        .route("/auth/me", get(me))
        .route("/auth/tokens", get(list_tokens).post(create_token))
        .route("/auth/tokens/{id}", delete(revoke_token))
}

/// An API token as it is listed afterwards: everything except the token itself, which only
/// exists in the response to the call that created it.
#[derive(Serialize, sqlx::FromRow)]
pub struct ApiTokenRow {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Deserialize)]
pub struct NewToken {
    pub name: String,
}

/// The caller's own tokens, newest first. A token is never listed to anyone but its owner, and
/// an admin has no view of anyone else's: an admin can already reset a password, which revokes
/// them, and that is a visible act rather than a silent read.
async fn list_tokens(user: AuthUser, State(state): State<App>) -> Result<Json<Vec<ApiTokenRow>>, AppError> {
    Ok(Json(
        sqlx::query_as::<_, ApiTokenRow>(
            "SELECT id, name, prefix, created_at, last_used_at FROM api_tokens              WHERE user_id = $1 ORDER BY id DESC",
        )
        .bind(user.id)
        .fetch_all(&state.db)
        .await?,
    ))
}

/// Issues a token, returning the plaintext exactly once. Only the hash is stored, so there is
/// no second chance to read it and no way for anyone with the database to recover it.
///
/// `SessionUser`: this needs an interactive login, never a token. See the type's own comment --
/// a token that can mint tokens is a leak that repairs itself faster than its owner can notice.
async fn create_token(
    SessionUser(user): SessionUser,
    State(state): State<App>,
    Json(body): Json<NewToken>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(AppError::BadRequest("name must be 1 to 64 characters".into()));
    }
    let token = auth::new_api_token();
    let id: (i64,) = sqlx::query_as(
        "INSERT INTO api_tokens (user_id, name, token_hash, prefix, created_at)          VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(user.id)
    .bind(name)
    .bind(auth::hash_api_token(&token))
    .bind(auth::token_prefix(&token))
    .bind(db::now())
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(json!({
        "id": id.0,
        "name": name,
        "prefix": auth::token_prefix(&token),
        "created_at": db::now(),
        // The only time this is ever readable.
        "token": token,
    }))))
}

/// Revoking takes effect on the next request: nothing caches the lookup.
///
/// A session may revoke any of its own tokens, exactly as before. A bearer token may
/// additionally revoke ITSELF: `AuthUser::token_id` carries the id of the token that
/// authenticated this request, and a caller authenticated that way is allowed through only when
/// it names that same id. This does not weaken `create_token`'s session-only rule -- a token
/// that could mint replacements would be a leak that repairs itself, but a token that can end
/// only its own validity can make itself no more dangerous than a leaked token already is; at
/// most it lets the app that holds it sign itself out.
///
/// A token naming any OTHER id -- including one it does not own -- gets exactly the refusal a
/// token caller has always gotten from this route: today that is `SessionUser` rejecting the
/// request outright with `AppError::Unauthorized` before a handler ever runs, so that is what a
/// mismatched id gets here too, rather than the `NotFound` a session caller gets for someone
/// else's id below. `tests/openapi.rs` and `tests/api_tokens.rs` pin both refusals.
async fn revoke_token(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    if user.token_id.is_some_and(|token_id| token_id != id) {
        return Err(AppError::Unauthorized);
    }
    let done = sqlx::query("DELETE FROM api_tokens WHERE id = $1 AND user_id = $2")
        .bind(id).bind(user.id)
        .execute(&state.db).await?;
    // Someone else's token is reported as absent rather than forbidden: whether an id exists is
    // not this caller's business either way.
    if done.rows_affected() == 0 { Err(AppError::NotFound) } else { Ok(StatusCode::NO_CONTENT) }
}

/// `Clear-Site-Data` header name. The `http` crate only special-cases the ~70 headers RFC-listed
/// as "standard"; this one isn't among them, so there is no `header::CLEAR_SITE_DATA` constant
/// to reuse.
const CLEAR_SITE_DATA: HeaderName = HeaderName::from_static("clear-site-data");

/// Sent on every response that starts or ends a session (setup, login, logout, logout-all).
/// Before 3555016, `/api/files/{id}` and its thumbnails were served `private, max-age=31536000,
/// immutable`, so a browser that already cached those bytes will keep reusing them for up to a
/// year without ever asking the server again -- on a shared browser, the next person to sign in
/// could still open the previous user's files straight from that cache. `"cache"` drops exactly
/// that HTTP cache. It is deliberately not `"storage"`, which would also wipe the offline
/// outbox in IndexedDB and the app's own `localStorage`, and not `"cookies"`, which would sign
/// the browser back out right after signing it in.
const CLEAR_SITE_DATA_CACHE: &str = "\"cache\"";

/// The one-element header array added to every successful setup/login/logout/logout-all
/// response.
fn clear_site_data() -> [(HeaderName, HeaderValue); 1] {
    [(CLEAR_SITE_DATA, HeaderValue::from_static(CLEAR_SITE_DATA_CACHE))]
}

#[derive(Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    /// The browser's IANA timezone, sent by first-run setup. Ignored by login, and by setup
    /// when `LOGB_TIMEZONE` is set or the value is not a timezone.
    #[serde(default)]
    pub timezone: Option<String>,
}

async fn user_count(state: &App) -> Result<i64, AppError> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users").fetch_one(&state.db).await?;
    Ok(n)
}

async fn status(State(state): State<App>) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(json!({ "setup_required": user_count(&state).await? == 0 })))
}

async fn setup(
    State(state): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<Credentials>,
) -> Result<(StatusCode, [(HeaderName, HeaderValue); 1], CookieJar, Json<AuthUser>), AppError> {
    // Cheap pre-check: rejects the common "setup already done" call before spending an
    // Argon2 hash on it. The authoritative guard is the transaction below.
    if user_count(&state).await? > 0 {
        return Err(AppError::Conflict("setup already completed".into()));
    }
    auth::validate_username(&body.username)?;
    auth::validate_password(&body.password)?;
    // Hashed before the write transaction opens: Argon2 takes hundreds of milliseconds, and
    // doing it here means it happens outside the `begin_write` lock instead of holding
    // PostgreSQL's advisory lock -- and every other writer in the app -- for the duration.
    let hash = auth::hash_password(&body.password)?;
    // Count, conditional insert and session creation all happen inside one `db::begin_write`
    // transaction, which on PostgreSQL holds the same advisory lock every other write path
    // takes (see `db::begin_write`). That -- not the `WHERE NOT EXISTS` below -- is what stops
    // two concurrent setup calls from both becoming admin: PostgreSQL's own MVCC would let
    // both transactions see an empty `users` table and both insert under READ COMMITTED
    // without it. `WHERE NOT EXISTS` is kept because it costs nothing and still states the
    // intent in the statement that depends on it, but it is no longer what makes this safe.
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let user = sqlx::query_as::<_, AuthUser>(
        "INSERT INTO users (username, password_hash, is_admin, lang, created_at) \
         SELECT $1, $2, 1, 'en', $3 WHERE NOT EXISTS (SELECT 1 FROM users) \
         RETURNING id, username, is_admin, lang",
    )
    .bind(&body.username).bind(hash).bind(db::now())
    .fetch_optional(&mut *tx).await?
    .ok_or_else(|| AppError::Conflict("setup already completed".into()))?;
    let token = auth::create_session_in(&mut tx, user.id).await?;
    tx.commit().await?;
    // The person setting the instance up is almost always sitting in the timezone it is for,
    // and UTC -- the only other guess available -- makes reminders come due at the wrong
    // midnight for most of the world. A value that does not parse is dropped rather than
    // refused: a first run must not fail over a timezone the browser spelled oddly.
    if state.config.timezone.is_none() {
        if let Some(tz) = body.timezone.as_deref().and_then(|s| super::settings::parse_timezone(s).ok()) {
            super::settings::store_timezone(&state, tz).await?;
        }
    }
    let jar = jar.add(auth::session_cookie(token, auth::wants_secure(&state, &headers)));
    Ok((StatusCode::CREATED, clear_site_data(), jar, Json(user)))
}

async fn login(
    State(state): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<Credentials>,
) -> Result<([(HeaderName, HeaderValue); 1], CookieJar, Json<AuthUser>), AppError> {
    auth::check_login_rate(&state, auth::client_ip(&state, &headers, peer))?;
    // Case-insensitive by `lower(...)` on both sides rather than by the column's collation:
    // SQLite declares `UNIQUE COLLATE NOCASE`, PostgreSQL carries a unique index on
    // `lower(username)`, and only this spelling signs "bEn" in as "Ben" on both.
    let row: Option<(i64, String)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE lower(username) = lower($1)")
        .bind(&body.username)
        .fetch_optional(&state.db).await?;
    let Some((id, hash)) = row else {
        // No such user: still do a full Argon2 verification so this path takes about as long
        // as the "wrong password" path below, and the two can't be told apart by timing.
        auth::verify_dummy_password(&body.password);
        return Err(AppError::Unauthorized);
    };
    if !auth::verify_password(&body.password, &hash) {
        return Err(AppError::Unauthorized);
    }
    let user = sqlx::query_as::<_, AuthUser>("SELECT id, username, is_admin, lang FROM users WHERE id = $1")
        .bind(id).fetch_one(&state.db).await?;
    let token = auth::create_session(&state, user.id).await?;
    let jar = jar.add(auth::session_cookie(token, auth::wants_secure(&state, &headers)));
    Ok((clear_site_data(), jar, Json(user)))
}

async fn logout(
    State(state): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<(StatusCode, [(HeaderName, HeaderValue); 1], CookieJar), AppError> {
    if let Some(c) = jar.get(auth::COOKIE) {
        auth::delete_session(&state, c.value()).await?;
    }
    let secure = auth::wants_secure(&state, &headers);
    Ok((StatusCode::NO_CONTENT, clear_site_data(), jar.remove(auth::removal_cookie(secure))))
}

/// Ends every session of the caller, this browser's included -- the "signed in somewhere I
/// don't recognise" button.
async fn logout_all(
    user: AuthUser,
    State(state): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<(StatusCode, [(HeaderName, HeaderValue); 1], CookieJar), AppError> {
    auth::delete_sessions_for_user(&state, user.id).await?;
    let secure = auth::wants_secure(&state, &headers);
    Ok((StatusCode::NO_CONTENT, clear_site_data(), jar.remove(auth::removal_cookie(secure))))
}

async fn me(user: AuthUser) -> Json<AuthUser> {
    Json(user)
}
