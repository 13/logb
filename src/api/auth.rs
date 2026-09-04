use crate::auth::{self, AuthUser};
use crate::db;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;

pub fn router() -> Router<App> {
    Router::new()
        .route("/auth/status", get(status))
        .route("/auth/setup", post(setup))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
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
    if user_count(&state).await? > 0 {
        return Err(AppError::Conflict("setup already completed".into()));
    }
    auth::validate_username(&body.username)?;
    auth::validate_password(&body.password)?;
    let hash = auth::hash_password(&body.password)?;
    let user = sqlx::query_as::<_, AuthUser>(
        "INSERT INTO users (username, password_hash, is_admin, lang, created_at) VALUES (?, ?, 1, 'en', ?) \
         RETURNING id, username, is_admin, lang",
    )
    .bind(&body.username).bind(hash).bind(db::now())
    .fetch_one(&state.db).await?;
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
    auth::check_login_rate(&state, auth::client_ip(&headers, peer))?;
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

async fn logout(State(state): State<App>, jar: CookieJar) -> Result<(StatusCode, CookieJar), AppError> {
    if let Some(c) = jar.get(auth::COOKIE) {
        auth::delete_session(&state, c.value()).await?;
    }
    Ok((StatusCode::NO_CONTENT, jar.remove(auth::removal_cookie())))
}

async fn me(user: AuthUser) -> Json<AuthUser> {
    Json(user)
}
