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
    /// The id of the API token that authenticated this request, or `None` for a session
    /// (cookie) caller. `#[serde(skip)]` so it never reaches a response body -- notably
    /// `/auth/me`, which returns this struct directly -- and `#[sqlx(default)]` so every query
    /// that does not select a `token_id` column (every session lookup, and every place this
    /// struct was already built before this field existed) still populates it as `None` rather
    /// than failing to decode. It exists purely so `revoke_token` can check that a token caller
    /// is revoking only itself; no other handler reads it.
    #[serde(skip)]
    #[sqlx(default)]
    pub token_id: Option<i64>,
    /// `users.notify_tz`, the zone the user chose for their digest hour and, from the same
    /// setting, for reading their reminders' due dates. `#[serde(skip)]` because `/auth/me`
    /// returns this struct and the notifications endpoint already reports the zone;
    /// `#[sqlx(default)]` so a query that does not select it still decodes.
    #[serde(skip)]
    #[sqlx(default)]
    pub tz: Option<String>,
}

impl AuthUser {
    /// Today in this user's own zone (`db::zone`), the date their reminders are due against.
    pub fn today(&self) -> chrono::NaiveDate {
        crate::db::today_in(self.tz.as_deref())
    }
}

pub struct AdminUser(pub AuthUser);

/// Hashes a password with Argon2, on the blocking thread pool.
///
/// Argon2 is deliberately slow -- tens to hundreds of milliseconds of pure CPU, and 19 MiB of
/// memory, per call. Run inline, that time is taken from a Tokio worker thread, which then
/// serves no other request until it finishes: a handful of concurrent sign-ins could stall
/// every other request on the instance, and a flood of wrong passwords would be a cheap way to
/// do it on purpose. `spawn_blocking` moves the work to the pool that exists for exactly this.
pub async fn hash_password(password: &str) -> Result<String, AppError> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| AppError::Internal(format!("hash: {e}")))
    })
    .await
    .map_err(|e| AppError::Internal(format!("hash task: {e}")))?
}

/// Checks `password` against a stored PHC hash, on the blocking thread pool for the same
/// reason as `hash_password`. A hash that does not parse, or a task that did not finish,
/// verifies nothing: both answer `false`, never an error a caller could mistake for success.
pub async fn verify_password(password: &str, hash: &str) -> bool {
    let (password, hash) = (password.to_owned(), hash.to_owned());
    tokio::task::spawn_blocking(move || verify_password_blocking(&password, &hash))
        .await
        .unwrap_or(false)
}

fn verify_password_blocking(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
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
pub async fn verify_dummy_password(password: &str) {
    verify_password(password, DUMMY_PASSWORD_HASH).await;
}

pub fn validate_username(u: &str) -> Result<(), AppError> {
    let ok = (3..=32).contains(&u.len())
        && u.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "username must be 3-32 chars of letters, digits, _ . -".into(),
        ))
    }
}

pub fn validate_password(p: &str) -> Result<(), AppError> {
    if p.len() >= 8 {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "password must be at least 8 characters".into(),
        ))
    }
}

pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    hex::encode(bytes)
}

/// What `sessions.token` holds for a session whose cookie carries `token`: its SHA-256, hex.
///
/// The database never sees the token itself, so a copy of it -- a nightly snapshot, a backup
/// archive, a disk that walked off -- is not a stack of cookies anyone can replay. A fast hash
/// rather than a password hash, for the reason `hash_api_token` gives: the token is 256 random
/// bits nobody chose, so there is nothing for a dictionary to attack, and the lookup runs on
/// every request. Existing plaintext rows were deleted by migration 0027 (SQLite) / 0018
/// (PostgreSQL) rather than converted.
pub fn hash_session_token(token: &str) -> String {
    crate::files::sha256_hex(token.as_bytes())
}

/// Opens a session for `user_id` and returns the token for its cookie. Only
/// `hash_session_token(token)` is stored.
pub async fn create_session(state: &App, user_id: i64) -> Result<String, AppError> {
    let token = new_token();
    let expires = (chrono::Utc::now() + chrono::Duration::days(SESSION_DAYS))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
        .bind(db::now())
        .execute(&state.db)
        .await?;
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(hash_session_token(&token))
        .bind(user_id)
        .bind(expires)
        .execute(&state.db)
        .await?;
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
    sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
        .bind(db::now())
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(hash_session_token(&token))
        .bind(user_id)
        .bind(expires)
        .execute(&mut **tx)
        .await?;
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
///
/// Both deletes commit together or not at all: its own write transaction, so a failure between
/// them cannot leave the sessions gone and the tokens -- the credential that matters more --
/// still working.
pub async fn delete_sessions_for_user(state: &App, user_id: i64) -> Result<(), AppError> {
    let mut tx = db::begin_write(state).await?;
    delete_sessions_for_user_in(&mut tx, user_id).await?;
    tx.commit().await?;
    Ok(())
}

/// As `delete_sessions_for_user`, inside a write transaction the caller already holds -- for a
/// caller whose other statements must commit with these, like a password change
/// (`api::users::update`). See `create_session_in` for why it must not take a pool connection
/// of its own.
pub async fn delete_sessions_for_user_in(
    tx: &mut sqlx::Transaction<'static, sqlx::Any>,
    user_id: i64,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM api_tokens WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Ends the session whose cookie carries `token` (the plaintext, as the browser sent it).
pub async fn delete_session(state: &App, token: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE token = $1")
        .bind(hash_session_token(token))
        .execute(&state.db)
        .await?;
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

/// The socket peer address, or the rightmost `X-Forwarded-For` hop when the deployment is
/// configured to trust a proxy. Without that flag the header is ignored, so a client cannot
/// spoof its way past the login rate limit.
///
/// Rightmost, not first: a proxy that appends (nginx's `$proxy_add_x_forwarded_for`, Traefik,
/// Caddy) adds the address it saw to the END of whatever the client sent, so every entry to the
/// left of the last one is client-supplied. Reading the first hop let a client put a fresh
/// made-up address at the front of each request and never meet the login limit. A proxy that
/// overwrites the header leaves one entry, which is both first and last, so nothing changes for
/// it. When the header arrives as several lines rather than one comma-separated value, the last
/// line is the last hop for the same reason.
pub fn client_ip(state: &App, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    if !state.config.trust_proxy {
        return peer.ip();
    }
    last_forwarded(headers, "x-forwarded-for")
        .and_then(|v| v.parse().ok())
        .unwrap_or(peer.ip())
}

/// The last comma-separated entry of the last `name` header line, trimmed: the one hop a
/// trusted proxy wrote itself (see `client_ip`). `None` when the header is absent, not text, or
/// its last entry is blank.
pub fn last_forwarded<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(name)
        .iter()
        .next_back()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// Returns Err(TooManyRequests) once an IP exceeds `login_max_attempts` inside LOGIN_WINDOW.
///
/// Every call first drops entries whose window has already elapsed, so the map only ever
/// holds IPs that attempted a login within the last `LOGIN_WINDOW`. Without that sweep the
/// map grows once per distinct source address for the lifetime of the process — unbounded
/// memory, and remotely driveable when `LOGB_TRUST_PROXY` makes the key attacker-chosen.
pub fn check_login_rate(state: &App, ip: IpAddr) -> Result<(), AppError> {
    count_attempt(
        &mut state.login_attempts.lock().unwrap(),
        ip,
        state.config.login_max_attempts,
    )
}

/// The longest username key the per-username limiter keeps. `validate_username` caps a real
/// name at 32, and a login body can carry any string at all; without a cap, one request could
/// park an arbitrarily large key in memory for a whole `LOGIN_WINDOW`.
const USERNAME_KEY_MAX: usize = 64;

/// As `check_login_rate`, keyed on the lower-cased username instead of the address.
///
/// The per-IP limit alone does nothing against guesses at one account spread across many
/// addresses -- a botnet, or a client behind a proxy that reports a different hop each time.
/// This one is keyed on the account being guessed at, with the same window and maximum, and is
/// swept the same way. Lower-cased because sign-in is case-insensitive: "Ben" and "BEN" are
/// the same account and must spend the same allowance.
///
/// A username that does not exist is counted exactly like one that does. If only real accounts
/// could ever answer 429, the limiter itself would tell a caller which names are taken -- the
/// same leak `verify_dummy_password` closes for timing.
///
/// The cost is that anyone can use up a named account's allowance for a minute. That is the
/// trade every per-account limit makes, and a minute is short.
pub fn check_username_rate(state: &App, username: &str) -> Result<(), AppError> {
    let key: String = username.to_lowercase().chars().take(USERNAME_KEY_MAX).collect();
    count_attempt(
        &mut state.login_attempts_by_user.lock().unwrap(),
        key,
        state.config.login_max_attempts,
    )
}

/// One attempt against `key` in a fixed-window limiter: sweep expired windows, count this
/// attempt, and refuse it once the window holds more than `max`. Shared by both limiters so the
/// per-IP and per-username rules cannot drift apart.
fn count_attempt<K: Eq + std::hash::Hash>(
    map: &mut std::collections::HashMap<K, (u32, Instant)>,
    key: K,
    max: u32,
) -> Result<(), AppError> {
    let now = Instant::now();
    map.retain(|k, &mut (_, started)| *k == key || now.duration_since(started) <= LOGIN_WINDOW);
    let entry = map.entry(key).or_insert((0, now));
    if now.duration_since(entry.1) > LOGIN_WINDOW {
        *entry = (0, now);
    }
    entry.0 += 1;
    if entry.0 > max {
        Err(AppError::TooManyRequests)
    } else {
        Ok(())
    }
}

pub fn token_from_parts(parts: &Parts) -> Option<String> {
    CookieJar::from_headers(&parts.headers)
        .get(COOKIE)
        .map(|c| c.value().to_string())
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
    let raw = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, value) = raw.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Resolves a bearer token to its owner, and records that it was used.
///
/// `last_used_at` is written at most once a day per token rather than on every request: it
/// exists so a user can recognise a token they no longer need, which day-level accuracy answers
/// perfectly well, and a write per request would turn every read of the API into a write.
async fn user_for_api_token(state: &App, token: &str) -> Result<Option<AuthUser>, AppError> {
    let hash = hash_api_token(token);
    // `t.id AS token_id` is the only reason this differs from the session lookup below: it
    // lands in `AuthUser::token_id` by column name, so a caller authenticated this way carries
    // the id of the very token it used.
    let row = sqlx::query_as::<_, AuthUser>(
        "SELECT u.id, u.username, u.is_admin, u.lang, u.notify_tz AS tz, t.id AS token_id \
         FROM api_tokens t JOIN users u ON u.id = t.user_id WHERE t.token_hash = $1",
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
        .bind(db::now())
        .bind(&hash)
        .bind(&today)
        .execute(&state.db)
        .await?;
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
            "SELECT u.id, u.username, u.is_admin, u.lang, u.notify_tz AS tz FROM sessions s \
             JOIN users u ON u.id = s.user_id WHERE s.token = $1 AND s.expires_at > $2",
        )
        .bind(hash_session_token(&token))
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
            "SELECT u.id, u.username, u.is_admin, u.lang, u.notify_tz AS tz FROM sessions s \
             JOIN users u ON u.id = s.user_id WHERE s.token = $1 AND s.expires_at > $2",
        )
        .bind(hash_session_token(&token))
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
        if user.is_admin.0 {
            Ok(AdminUser(user))
        } else {
            Err(AppError::Forbidden)
        }
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
        assert!(
            PasswordHash::new(DUMMY_PASSWORD_HASH).is_ok(),
            "DUMMY_PASSWORD_HASH must be a valid PHC string"
        );
        assert!(!verify_password_blocking(
            "definitely-not-the-password",
            DUMMY_PASSWORD_HASH
        ));
    }

    async fn test_state(trust_proxy: bool) -> App {
        let dir = tempfile::tempdir().unwrap();
        let url = db::sqlite_url(dir.path()).unwrap();
        let db = db::connect(&url).await.unwrap();
        let storage = crate::files::Storage::new(dir.path()).unwrap();
        let config = Config {
            data_dir: dir.path().to_path_buf(),
            bind: "127.0.0.1".into(),
            port: 0,
            max_upload_mb: 2,
            max_import_mb: 4,
            notify_url: None,
            telegram_bot_token: None,
            telegram_api_url: "https://api.telegram.org".into(),
            notify_hour: 8,
            notify_format: "json".into(),
            public_url: None,
            timezone: None,
            backup: None,
            backup_dir: None,
            backup_hour: 3,
            restore: None,
            copy_to: None,
            healthcheck: false,
            secure_cookie: "false".into(),
            log: "warn".into(),
            trust_proxy,
            login_max_attempts: 10,
            cors_origins: String::new(),
            database_url: None,
            db_pool_size: None,
            allow_loopback_http_push: false,
        };
        Arc::new(AppState {
            write_db: db.clone(),
            db,
            database_url: url,
            backend: crate::dialect::Backend::Sqlite,
            storage,
            config,
            login_attempts: Mutex::new(HashMap::new()),
            login_attempts_by_user: Mutex::new(HashMap::new()),
            backup_verified: Mutex::new(None),
            shutdown: tokio_util::sync::CancellationToken::new(),
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

    /// The trusted proxy APPENDS the address it saw to whatever `X-Forwarded-For` the client
    /// sent, so the rightmost entry is the only one the proxy vouches for. Everything to its left
    /// is client-supplied.
    #[tokio::test]
    async fn client_ip_uses_the_rightmost_hop_when_trust_proxy_is_on() {
        let state = test_state(true).await;
        let headers = headers_with_xff("198.51.100.7, 10.0.0.1");
        assert_eq!(
            client_ip(&state, &headers, peer()),
            "10.0.0.1".parse::<IpAddr>().unwrap()
        );
    }

    /// Regression: `client_ip` used to read the FIRST hop, so a client behind a trusted proxy
    /// could put a fresh made-up address at the front of `X-Forwarded-For` on every attempt and
    /// never meet the login limit. Rotating the leading hop must not reset anything.
    #[tokio::test]
    async fn a_spoofed_leading_hop_does_not_escape_the_login_limit() {
        let state = test_state(true).await;
        let max = state.config.login_max_attempts;
        let mut results = Vec::new();
        for i in 0..=max {
            let headers = headers_with_xff(&format!("192.0.2.{i}, 198.51.100.7"));
            results.push(check_login_rate(&state, client_ip(&state, &headers, peer())).is_ok());
        }
        assert!(results[..max as usize].iter().all(|ok| *ok), "{results:?}");
        assert!(!results[max as usize], "attempt {} must be refused", max + 1);
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

    /// The username map is swept exactly like the address map, and its keys are capped, so a
    /// flood of made-up names cannot grow it past what one window holds.
    #[tokio::test]
    async fn username_rate_map_is_swept_and_its_keys_capped() {
        let state = test_state(false).await;
        {
            let mut map = state.login_attempts_by_user.lock().unwrap();
            let stale = Instant::now() - LOGIN_WINDOW * 3;
            for i in 0..50 {
                map.insert(format!("ghost{i}"), (1, stale));
            }
        }
        check_username_rate(&state, &"X".repeat(10_000)).unwrap();
        let map = state.login_attempts_by_user.lock().unwrap();
        assert_eq!(map.len(), 1, "expired entries must not accumulate");
        let key = map.keys().next().unwrap();
        assert_eq!(key.len(), USERNAME_KEY_MAX);
        assert!(key.chars().all(|c| c == 'x'), "keys are lower-cased");
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
