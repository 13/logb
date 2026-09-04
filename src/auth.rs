use crate::db;
use crate::error::AppError;
use crate::state::App;
use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use rand::RngExt;
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

pub const COOKIE: &str = "memto_session";
const SESSION_DAYS: i64 = 30;
const LOGIN_MAX_ATTEMPTS: u32 = 10;
const LOGIN_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub lang: String,
}

pub struct AdminUser(pub AuthUser);

pub fn hash_password(password: &str) -> Result<String, AppError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("hash: {e}")))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

pub fn validate_username(u: &str) -> Result<(), AppError> {
    let ok = (3..=32).contains(&u.len())
        && u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok { Ok(()) } else {
        Err(AppError::BadRequest("username must be 3-32 chars of letters, digits, _ . -".into()))
    }
}

pub fn validate_password(p: &str) -> Result<(), AppError> {
    if p.len() >= 8 { Ok(()) } else { Err(AppError::BadRequest("password must be at least 8 characters".into())) }
}

pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    hex::encode(bytes)
}

pub async fn create_session(state: &App, user_id: i64) -> Result<String, AppError> {
    let token = new_token();
    let expires = (chrono::Utc::now() + chrono::Duration::days(SESSION_DAYS))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?").bind(db::now()).execute(&state.db).await?;
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES (?, ?, ?)")
        .bind(&token).bind(user_id).bind(expires)
        .execute(&state.db).await?;
    Ok(token)
}

pub async fn delete_session(state: &App, token: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE token = ?").bind(token).execute(&state.db).await?;
    Ok(())
}

/// Secure flag: config `true`/`false`, or `auto` = behind an https reverse proxy.
pub fn wants_secure(state: &App, headers: &HeaderMap) -> bool {
    match state.config.secure_cookie.as_str() {
        "true" => true,
        "false" => false,
        _ => headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("https"))
            .unwrap_or(false),
    }
}

pub fn session_cookie(token: String, secure: bool) -> Cookie<'static> {
    Cookie::build((COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(time::Duration::days(SESSION_DAYS))
        .build()
}

pub fn removal_cookie() -> Cookie<'static> {
    Cookie::build((COOKIE, ""))
        .path("/")
        .http_only(true)
        .max_age(time::Duration::ZERO)
        .build()
}

/// First `X-Forwarded-For` hop if present, else the socket peer address.
pub fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(peer.ip())
}

/// Returns Err(TooManyRequests) once an IP exceeds LOGIN_MAX_ATTEMPTS inside LOGIN_WINDOW.
pub fn check_login_rate(state: &App, ip: IpAddr) -> Result<(), AppError> {
    let mut map = state.login_attempts.lock().unwrap();
    let now = Instant::now();
    let entry = map.entry(ip).or_insert((0, now));
    if now.duration_since(entry.1) > LOGIN_WINDOW {
        *entry = (0, now);
    }
    entry.0 += 1;
    if entry.0 > LOGIN_MAX_ATTEMPTS { Err(AppError::TooManyRequests) } else { Ok(()) }
}

pub fn token_from_parts(parts: &Parts) -> Option<String> {
    CookieJar::from_headers(&parts.headers).get(COOKIE).map(|c| c.value().to_string())
}

impl FromRequestParts<App> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &App) -> Result<Self, AppError> {
        let token = token_from_parts(parts).ok_or(AppError::Unauthorized)?;
        sqlx::query_as::<_, AuthUser>(
            "SELECT u.id, u.username, u.is_admin, u.lang FROM sessions s \
             JOIN users u ON u.id = s.user_id WHERE s.token = ? AND s.expires_at > ?",
        )
        .bind(token)
        .bind(db::now())
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::Unauthorized)
    }
}

impl FromRequestParts<App> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &App) -> Result<Self, AppError> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.is_admin { Ok(AdminUser(user)) } else { Err(AppError::Forbidden) }
    }
}
