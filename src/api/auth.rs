use crate::auth::{self, AuthUser, SessionUser};
use crate::db;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
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
            "SELECT id, name, prefix, created_at, last_used_at FROM api_tokens              WHERE user_id = ? ORDER BY id DESC",
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
        "INSERT INTO api_tokens (user_id, name, token_hash, prefix, created_at)          VALUES (?, ?, ?, ?, ?) RETURNING id",
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
async fn revoke_token(
    SessionUser(user): SessionUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let done = sqlx::query("DELETE FROM api_tokens WHERE id = ? AND user_id = ?")
        .bind(id).bind(user.id)
        .execute(&state.db).await?;
    // Someone else's token is reported as absent rather than forbidden: whether an id exists is
    // not this caller's business either way.
    if done.rows_affected() == 0 { Err(AppError::NotFound) } else { Ok(StatusCode::NO_CONTENT) }
}

#[derive(Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
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
) -> Result<(StatusCode, CookieJar, Json<AuthUser>), AppError> {
    // Cheap pre-check: rejects the common "setup already done" call before spending an
    // Argon2 hash on it. The authoritative guard is the conditional INSERT below.
    if user_count(&state).await? > 0 {
        return Err(AppError::Conflict("setup already completed".into()));
    }
    auth::validate_username(&body.username)?;
    auth::validate_password(&body.password)?;
    let hash = auth::hash_password(&body.password)?;
    // `WHERE NOT EXISTS (SELECT 1 FROM users)` re-checks emptiness inside the same statement
    // that writes the row, so two concurrent setup calls with different usernames cannot both
    // pass the check and both become admin — the loser inserts nothing and gets a 409.
    let user = sqlx::query_as::<_, AuthUser>(
        "INSERT INTO users (username, password_hash, is_admin, lang, created_at) \
         SELECT ?, ?, 1, 'en', ? WHERE NOT EXISTS (SELECT 1 FROM users) \
         RETURNING id, username, is_admin, lang",
    )
    .bind(&body.username).bind(hash).bind(db::now())
    .fetch_optional(&state.db).await?
    .ok_or_else(|| AppError::Conflict("setup already completed".into()))?;
    let token = auth::create_session(&state, user.id).await?;
    let jar = jar.add(auth::session_cookie(token, auth::wants_secure(&state, &headers)));
    Ok((StatusCode::CREATED, jar, Json(user)))
}

async fn login(
    State(state): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<Credentials>,
) -> Result<(CookieJar, Json<AuthUser>), AppError> {
    auth::check_login_rate(&state, auth::client_ip(&state, &headers, peer))?;
    let row: Option<(i64, String)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE username = ?")
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
    let user = sqlx::query_as::<_, AuthUser>("SELECT id, username, is_admin, lang FROM users WHERE id = ?")
        .bind(id).fetch_one(&state.db).await?;
    let token = auth::create_session(&state, user.id).await?;
    let jar = jar.add(auth::session_cookie(token, auth::wants_secure(&state, &headers)));
    Ok((jar, Json(user)))
}

async fn logout(
    State(state): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<(StatusCode, CookieJar), AppError> {
    if let Some(c) = jar.get(auth::COOKIE) {
        auth::delete_session(&state, c.value()).await?;
    }
    let secure = auth::wants_secure(&state, &headers);
    Ok((StatusCode::NO_CONTENT, jar.remove(auth::removal_cookie(secure))))
}

/// Ends every session of the caller, this browser's included -- the "signed in somewhere I
/// don't recognise" button.
async fn logout_all(
    user: AuthUser,
    State(state): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<(StatusCode, CookieJar), AppError> {
    auth::delete_sessions_for_user(&state, user.id).await?;
    let secure = auth::wants_secure(&state, &headers);
    Ok((StatusCode::NO_CONTENT, jar.remove(auth::removal_cookie(secure))))
}

async fn me(user: AuthUser) -> Json<AuthUser> {
    Json(user)
}
