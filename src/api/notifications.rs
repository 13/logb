//! A person's own notification settings: where their digest goes, and the browsers that get it
//! as a push notification.

use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::notify;
use crate::push::{self, Subscription};
use crate::state::App;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<App> {
    Router::new()
        .route("/me/notifications", get(read).put(write))
        .route("/me/notifications/test", post(test))
        .route("/push/subscriptions", post(subscribe).delete(unsubscribe))
}

#[derive(Serialize)]
pub struct NotificationsOut {
    /// This user's own webhook. Null keeps them in the instance digest, when there is one.
    pub url: Option<String>,
    /// `text` (what ntfy renders) or `json`.
    pub format: String,
    /// How many browsers this user has turned push notifications on in.
    pub push_devices: i64,
    /// What a browser subscribes with: this instance's VAPID public key, base64url.
    pub vapid_public_key: String,
    /// Whether `LOGB_NOTIFY_URL` is set. While `url` is null, this user's reminders are in it.
    pub instance_webhook: bool,
    /// The hour, in the instance's timezone, the daily digest goes out.
    pub hour: u32,
}

async fn out(state: &App, user_id: i64) -> Result<NotificationsOut, AppError> {
    let (url, format): (Option<String>, String) =
        sqlx::query_as("SELECT notify_url, notify_format FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await?;
    let (push_devices,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM push_subscriptions WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await?;
    let kp = push::key_pair(state).await?;
    Ok(NotificationsOut {
        url,
        format,
        push_devices,
        vapid_public_key: push::public_key(&kp),
        instance_webhook: state.config.notify_url.is_some(),
        hour: state.config.notify_hour,
    })
}

async fn read(
    user: AuthUser,
    State(state): State<App>,
) -> Result<Json<NotificationsOut>, AppError> {
    Ok(Json(out(&state, user.id).await?))
}

#[derive(Deserialize)]
pub struct NotificationsIn {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "text_format")]
    pub format: String,
}

fn text_format() -> String {
    "text".to_string()
}

/// A webhook is any http(s) address with a host; blank means none.
///
/// Plain http and private addresses are allowed on purpose: a self-hosted ntfy on the home
/// network is the likeliest target of all. The request is a fixed digest body and nothing it
/// answers is shown back to anyone, which is what keeps this from being a way to read internal
/// services.
pub(crate) fn validate_webhook(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let bad = || AppError::BadRequest("url must be an http or https address".into());
    if raw.len() > 2000 {
        return Err(bad());
    }
    let parsed = reqwest::Url::parse(raw).map_err(|_| bad())?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(bad());
    }
    Ok(Some(raw.to_string()))
}

async fn write(
    user: AuthUser,
    State(state): State<App>,
    Json(body): Json<NotificationsIn>,
) -> Result<Json<NotificationsOut>, AppError> {
    let url = validate_webhook(body.url.as_deref())?;
    if !matches!(body.format.as_str(), "json" | "text") {
        return Err(AppError::BadRequest("format must be json or text".into()));
    }
    sqlx::query("UPDATE users SET notify_url = $1, notify_format = $2 WHERE id = $3")
        .bind(&url)
        .bind(&body.format)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(Json(out(&state, user.id).await?))
}

#[derive(Serialize)]
pub struct TestOut {
    /// `sent`, the reason it failed, or null when this user has no webhook of their own.
    pub webhook: Option<String>,
    pub push_sent: usize,
    pub push_failed: usize,
}

/// Sends a test notification, now, to everywhere this user's own notifications go -- so they
/// can see a phone buzz instead of waiting until tomorrow's digest to find out it does not.
async fn test(user: AuthUser, State(state): State<App>) -> Result<Json<TestOut>, AppError> {
    let (url, format): (Option<String>, String) =
        sqlx::query_as("SELECT notify_url, notify_format FROM users WHERE id = $1")
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
    let digest = notify::test_digest(&user.lang);
    let webhook = match url {
        Some(url) => Some(match notify::post(&url, &format, &digest).await {
            Ok(()) => "sent".to_string(),
            Err(e) => e.to_string(),
        }),
        None => None,
    };
    let (push_sent, push_failed) = notify::push_to(
        &state,
        user.id,
        &digest.title,
        &digest.message,
        "/settings/notifications",
    )
    .await?;
    Ok(Json(TestOut {
        webhook,
        push_sent,
        push_failed,
    }))
}

#[derive(Deserialize)]
pub struct SubscriptionIn {
    pub endpoint: String,
    pub keys: SubscriptionKeys,
}

#[derive(Deserialize)]
pub struct SubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

async fn subscribe(
    user: AuthUser,
    State(state): State<App>,
    Json(body): Json<SubscriptionIn>,
) -> Result<StatusCode, AppError> {
    let sub = Subscription {
        endpoint: body.endpoint.trim().to_string(),
        p256dh: body.keys.p256dh.trim().to_string(),
        auth: body.keys.auth.trim().to_string(),
    };
    push::validate(&sub)?;
    // An endpoint names one browser profile. Subscribing it again refreshes its keys, and
    // another account signing in on that browser takes it over -- a browser only ever shows one
    // person's notifications, and it should be the person using it now.
    sqlx::query(
        "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth, created_at) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (endpoint) DO UPDATE SET user_id = excluded.user_id, p256dh = excluded.p256dh, auth = excluded.auth",
    )
    .bind(user.id).bind(&sub.endpoint).bind(&sub.p256dh).bind(&sub.auth).bind(db::now())
    .execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct EndpointIn {
    pub endpoint: String,
}

/// Idempotent, like `unsnooze`: an endpoint that is already gone is the state asked for.
async fn unsubscribe(
    user: AuthUser,
    State(state): State<App>,
    Json(body): Json<EndpointIn>,
) -> Result<StatusCode, AppError> {
    sqlx::query("DELETE FROM push_subscriptions WHERE endpoint = $1 AND user_id = $2")
        .bind(body.endpoint.trim())
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
