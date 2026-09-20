use crate::auth::{AdminUser, AuthUser};
use crate::db;
use crate::error::AppError;
use crate::state::App;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<App> {
    Router::new().route("/settings", get(read).put(write))
}

#[derive(Serialize)]
pub struct Settings {
    pub currency: String,
    /// The IANA timezone the instance runs on: which day a due date is read against, and when
    /// the daily digest goes out.
    pub timezone: String,
    /// True when `LOGB_TIMEZONE` decides it, which Settings cannot change.
    pub timezone_locked: bool,
}

#[derive(Deserialize)]
pub struct SettingsInput {
    pub currency: String,
    /// Optional, so a client that only knows about the currency keeps working.
    #[serde(default)]
    pub timezone: Option<String>,
}

pub async fn currency(state: &App) -> Result<String, AppError> {
    let (v,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'currency'")
        .fetch_one(&state.db)
        .await?;
    Ok(v)
}

async fn current(state: &App) -> Result<Settings, AppError> {
    Ok(Settings {
        currency: currency(state).await?,
        timezone: db::timezone().name().to_string(),
        timezone_locked: state.config.timezone.is_some(),
    })
}

/// Stores `tz` and switches the running instance to it.
pub async fn store_timezone(state: &App, tz: Tz) -> Result<(), AppError> {
    sqlx::query("INSERT INTO settings (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
        .bind(db::TIMEZONE_KEY).bind(tz.name()).execute(&state.db).await?;
    db::set_timezone(tz);
    Ok(())
}

pub fn parse_timezone(raw: &str) -> Result<Tz, AppError> {
    raw.trim().parse::<Tz>().map_err(|_| {
        AppError::BadRequest("timezone must be an IANA name like Europe/Berlin".into())
    })
}

async fn read(_: AuthUser, State(state): State<App>) -> Result<Json<Settings>, AppError> {
    Ok(Json(current(&state).await?))
}

async fn write(
    AdminUser(_): AdminUser,
    State(state): State<App>,
    Json(body): Json<SettingsInput>,
) -> Result<Json<Settings>, AppError> {
    let c = body.currency.trim();
    if c.len() != 3 || !c.chars().all(|ch| ch.is_ascii_uppercase()) {
        return Err(AppError::BadRequest(
            "currency must be a 3-letter ISO code like EUR".into(),
        ));
    }
    // Checked in full before anything is written, so a bad timezone does not leave a new
    // currency behind.
    let tz = match body.timezone.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => {
            let tz = parse_timezone(raw)?;
            // Compared with the configured value, not the process-wide `db::timezone()`: they are
            // the same in a running server, but tests start several apps in one process, and one
            // app's setup must not decide whether another app's locked timezone may change.
            if state.config.timezone.is_some_and(|locked| tz != locked) {
                return Err(AppError::BadRequest(
                    "the timezone is set by LOGB_TIMEZONE and cannot be changed here".into(),
                ));
            }
            Some(tz)
        }
        None => None,
    };
    sqlx::query("UPDATE settings SET value = $1 WHERE key = 'currency'")
        .bind(c)
        .execute(&state.db)
        .await?;
    if let (Some(tz), None) = (tz, state.config.timezone) {
        store_timezone(&state, tz).await?;
    }
    Ok(Json(current(&state).await?))
}
