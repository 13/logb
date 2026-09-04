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

/// A real Argon2 PHC hash of a fixed, never-used password, generated once with this crate's
/// own hasher (`Argon2::default().hash_password(..)`). It exists only so `verify_dummy_password`
/// can burn roughly the same time as a real `verify_password` call when a username doesn't
/// exist, so login can't be timed to tell "no such user" apart from "wrong password".
const DUMMY_PASSWORD_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$K0anc3UcBp8GE52U1zxCzw$4TOrZfpM/TiigOVMz3BMtOfr7nYLDPeV4VLD+buYL4Y";

/// Runs a full Argon2 verification against a fixed dummy hash so the "user not found" login
/// path costs about as much time as the "wrong password" path (see `DUMMY_PASSWORD_HASH`).
/// The result is always `false` and is not meant to be checked; only the timing matters.
pub fn verify_dummy_password(password: &str) {
    verify_password(password, DUMMY_PASSWORD_HASH);
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

/// The socket peer address, or the first `X-Forwarded-For` hop when the
/// deployment is configured to trust a proxy. Without that flag the header is
/// ignored, so a client cannot spoof its way past the login rate limit.
pub fn client_ip(state: &App, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    if !state.config.trust_proxy {
        return peer.ip();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::state::AppState;
    use axum::http::HeaderValue;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn dummy_password_hash_parses_and_rejects_wrong_password() {
        assert!(PasswordHash::new(DUMMY_PASSWORD_HASH).is_ok(), "DUMMY_PASSWORD_HASH must be a valid PHC string");
        assert!(!verify_password("definitely-not-the-password", DUMMY_PASSWORD_HASH));
    }

    async fn test_state(trust_proxy: bool) -> App {
        let dir = tempfile::tempdir().unwrap();
        let db = db::connect(dir.path()).await.unwrap();
        let config = Config {
            data_dir: dir.path().to_path_buf(),
            bind: "127.0.0.1".into(),
            port: 0,
            max_upload_mb: 2,
            secure_cookie: "false".into(),
            log: "warn".into(),
            trust_proxy,
        };
        Arc::new(AppState { db, config, login_attempts: Mutex::new(HashMap::new()) })
    }

    fn peer() -> SocketAddr {
        "203.0.113.9:12345".parse().unwrap()
    }

    fn headers_with_xff(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_str(value).unwrap());
        headers
    }

    #[tokio::test]
    async fn client_ip_ignores_header_when_trust_proxy_is_off() {
        let state = test_state(false).await;
        let headers = headers_with_xff("198.51.100.7");
        assert_eq!(client_ip(&state, &headers, peer()), peer().ip());
    }

    #[tokio::test]
    async fn client_ip_uses_first_hop_when_trust_proxy_is_on() {
        let state = test_state(true).await;
        let headers = headers_with_xff("198.51.100.7, 10.0.0.1");
        assert_eq!(client_ip(&state, &headers, peer()), "198.51.100.7".parse::<IpAddr>().unwrap());
    }

    #[tokio::test]
    async fn client_ip_falls_back_to_peer_when_trust_proxy_on_but_header_missing_or_unparseable() {
        let state = test_state(true).await;
        let no_header = HeaderMap::new();
        assert_eq!(client_ip(&state, &no_header, peer()), peer().ip());

        let bad_header = headers_with_xff("not-an-ip");
        assert_eq!(client_ip(&state, &bad_header, peer()), peer().ip());
    }
}
