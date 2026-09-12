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

pub const COOKIE: &str = "logb_session";
const SESSION_DAYS: i64 = 30;
const LOGIN_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub is_admin: crate::db::Bool,
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
    sqlx::query("DELETE FROM sessions WHERE expires_at <= $1").bind(db::now()).execute(&state.db).await?;
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(&token).bind(user_id).bind(expires)
        .execute(&state.db).await?;
    Ok(token)
}

/// As `create_session`, but runs on a write transaction the caller already holds instead of
/// asking the pool for a fresh connection.
///
/// Setup's admin-creation race (`api::auth::setup`) needs this rather than plain
/// `create_session`: with a pool as small as the test harness's two connections, a request
/// that already holds one connection open in an uncommitted transaction would block forever
/// asking the same pool for a second one to run `create_session` on -- a self-deadlock, not a
/// PostgreSQL lock wait, but one only visible once two such requests run at once.
pub async fn create_session_in(
    tx: &mut sqlx::Transaction<'static, sqlx::Any>,
    user_id: i64,
) -> Result<String, AppError> {
    let token = new_token();
    let expires = (chrono::Utc::now() + chrono::Duration::days(SESSION_DAYS))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    sqlx::query("DELETE FROM sessions WHERE expires_at <= $1").bind(db::now()).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(&token).bind(user_id).bind(expires)
        .execute(&mut **tx).await?;
    Ok(token)
}

/// Drops every session AND every API token belonging to `user_id`.
///
/// Called whenever a password changes: without it, a password reset -- the one action taken
/// precisely because an account may be compromised -- leaves any existing session valid for
/// the rest of its 30 days.
///
/// The same argument applies with more force to API tokens, which never expire at all: a reset
/// that left them alone would revoke the credential the owner can see and keep the one an
/// attacker actually took. The cost is that a password change signs the phone out of the API
/// too, which is why the README says so and the Settings screen says so next to the button.
pub async fn delete_sessions_for_user(state: &App, user_id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(user_id).execute(&state.db).await?;
    sqlx::query("DELETE FROM api_tokens WHERE user_id = $1").bind(user_id).execute(&state.db).await?;
    Ok(())
}

pub async fn delete_session(state: &App, token: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE token = $1").bind(token).execute(&state.db).await?;
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

/// Mirrors every attribute of `session_cookie` except the value and lifetime. Browsers match
/// a replacement cookie on name/path/domain, and reject a `Secure`-less overwrite of a
/// `Secure` cookie on a secure origin, so a deletion cookie that drops those attributes can
/// leave the live session cookie in place.
pub fn removal_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build((COOKIE, ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
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

/// Returns Err(TooManyRequests) once an IP exceeds `login_max_attempts` inside LOGIN_WINDOW.
///
/// Every call first drops entries whose window has already elapsed, so the map only ever
/// holds IPs that attempted a login within the last `LOGIN_WINDOW`. Without that sweep the
/// map grows once per distinct source address for the lifetime of the process — unbounded
/// memory, and remotely driveable when `LOGB_TRUST_PROXY` makes the key attacker-chosen.
pub fn check_login_rate(state: &App, ip: IpAddr) -> Result<(), AppError> {
    let mut map = state.login_attempts.lock().unwrap();
    let now = Instant::now();
    map.retain(|&addr, &mut (_, started)| addr == ip || now.duration_since(started) <= LOGIN_WINDOW);
    let entry = map.entry(ip).or_insert((0, now));
    if now.duration_since(entry.1) > LOGIN_WINDOW {
        *entry = (0, now);
    }
    entry.0 += 1;
    if entry.0 > state.config.login_max_attempts { Err(AppError::TooManyRequests) } else { Ok(()) }
}

pub fn token_from_parts(parts: &Parts) -> Option<String> {
    CookieJar::from_headers(&parts.headers).get(COOKIE).map(|c| c.value().to_string())
}

/// The prefix every API token carries, so one is recognisable on sight -- in a log, a config
/// file, a screenshot -- and can be revoked without having to work out what it is.
pub const TOKEN_PREFIX: &str = "logb_pat_";

/// How much of the plaintext is kept alongside the hash, purely so the token list can name the
/// row the user is looking at. Short enough to be useless for reconstructing the token.
const PREFIX_KEPT: usize = TOKEN_PREFIX.len() + 6;

pub fn new_api_token() -> String {
    format!("{TOKEN_PREFIX}{}", new_token())
}

/// Only the hash is stored (see migrations/sqlite/0006_api_tokens.sql). SHA-256 rather than a password
/// hash: this is a 256-bit random value, not something a user chose, so there is nothing for a
/// dictionary to attack and no reason to make verification -- which happens on every single
/// request -- deliberately slow.
pub fn hash_api_token(token: &str) -> String {
    crate::files::sha256_hex(token.as_bytes())
}

pub fn token_prefix(token: &str) -> String {
    token.chars().take(PREFIX_KEPT).collect()
}

/// A `Bearer` credential from the `Authorization` header, if there is one.
fn bearer_from_parts(parts: &Parts) -> Option<String> {
    let raw = parts.headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, value) = raw.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let value = value.trim();
    if value.is_empty() { None } else { Some(value.to_string()) }
}

/// Resolves a bearer token to its owner, and records that it was used.
///
/// `last_used_at` is written at most once a day per token rather than on every request: it
/// exists so a user can recognise a token they no longer need, which day-level accuracy answers
/// perfectly well, and a write per request would turn every read of the API into a write.
async fn user_for_api_token(state: &App, token: &str) -> Result<Option<AuthUser>, AppError> {
    let hash = hash_api_token(token);
    let row = sqlx::query_as::<_, AuthUser>(
        "SELECT u.id, u.username, u.is_admin, u.lang FROM api_tokens t \
         JOIN users u ON u.id = t.user_id WHERE t.token_hash = $1",
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;
    if row.is_some() {
        let today = db::today();
        sqlx::query(
            "UPDATE api_tokens SET last_used_at = $1 \
             WHERE token_hash = $2 AND (last_used_at IS NULL OR last_used_at < $3)",
        )
        .bind(db::now()).bind(&hash).bind(&today)
        .execute(&state.db).await?;
    }
    Ok(row)
}

/// A caller authenticated by a session COOKIE specifically, never by an API token.
///
/// Managing tokens is the one thing a token may not do. A leaked token is bad; a leaked token
/// that can mint more of itself, and revoke the ones its owner would use to notice, is worse --
/// and nothing legitimate needs it, since a client obtains its first token through an ordinary
/// interactive login.
pub struct SessionUser(pub AuthUser);

impl FromRequestParts<App> for SessionUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &App) -> Result<Self, AppError> {
        let token = token_from_parts(parts).ok_or(AppError::Unauthorized)?;
        let user = sqlx::query_as::<_, AuthUser>(
            "SELECT u.id, u.username, u.is_admin, u.lang FROM sessions s \
             JOIN users u ON u.id = s.user_id WHERE s.token = $1 AND s.expires_at > $2",
        )
        .bind(token)
        .bind(db::now())
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::Unauthorized)?;
        Ok(SessionUser(user))
    }
}

impl FromRequestParts<App> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &App) -> Result<Self, AppError> {
        // A bearer token first, since a client that sends one is saying which credential it
        // means -- and a native client may well be holding a stale cookie from the interactive
        // login it used to obtain that token in the first place.
        if let Some(bearer) = bearer_from_parts(parts) {
            if let Some(user) = user_for_api_token(state, &bearer).await? {
                return Ok(user);
            }
            return Err(AppError::Unauthorized);
        }
        let token = token_from_parts(parts).ok_or(AppError::Unauthorized)?;
        sqlx::query_as::<_, AuthUser>(
            "SELECT u.id, u.username, u.is_admin, u.lang FROM sessions s \
             JOIN users u ON u.id = s.user_id WHERE s.token = $1 AND s.expires_at > $2",
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
        if user.is_admin.0 { Ok(AdminUser(user)) } else { Err(AppError::Forbidden) }
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
        let db = db::connect(&db::sqlite_url(dir.path()).unwrap()).await.unwrap();
        let storage = crate::files::Storage::new(dir.path()).unwrap();
        let config = Config {
            data_dir: dir.path().to_path_buf(),
            bind: "127.0.0.1".into(),
            port: 0,
            max_upload_mb: 2,
        max_import_mb: 4,
        notify_url: None,
        notify_hour: 8,
        notify_format: "json".into(),
        timezone: chrono_tz::Tz::UTC,
        backup: None,
        backup_dir: None,
        backup_hour: 3,
        restore: None,
        healthcheck: false,
            secure_cookie: "false".into(),
            log: "warn".into(),
            trust_proxy,
            login_max_attempts: 10,
            cors_origins: String::new(),
            database_url: None,
            db_pool_size: None,
        };
        Arc::new(AppState {
            db,
            backend: crate::dialect::Backend::Sqlite,
            storage,
            config,
            login_attempts: Mutex::new(HashMap::new()),
        })
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
    async fn login_rate_map_drops_ips_whose_window_has_elapsed() {
        let state = test_state(false).await;
        {
            let mut map = state.login_attempts.lock().unwrap();
            let stale = Instant::now() - LOGIN_WINDOW * 3;
            for i in 0..50u8 {
                map.insert(IpAddr::from([198, 51, 100, i]), (1, stale));
            }
            assert_eq!(map.len(), 50);
        }
        check_login_rate(&state, "203.0.113.1".parse().unwrap()).unwrap();
        let map = state.login_attempts.lock().unwrap();
        assert_eq!(map.len(), 1, "expired entries must not accumulate");
    }

    #[tokio::test]
    async fn login_rate_map_keeps_ips_still_inside_their_window() {
        let state = test_state(false).await;
        check_login_rate(&state, "198.51.100.1".parse().unwrap()).unwrap();
        check_login_rate(&state, "198.51.100.2".parse().unwrap()).unwrap();
        assert_eq!(state.login_attempts.lock().unwrap().len(), 2);
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
