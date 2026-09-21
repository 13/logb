//! Telegram Bot API integration. The bot token belongs to the instance; users link one private
//! Telegram chat with a short-lived code from Settings.

use crate::db;
use crate::error::AppError;
use crate::notify::Digest;
use crate::state::App;
use chrono::{Duration as ChronoDuration, Utc};
use rand::RngExt;
use serde_json::{json, Value};
use sha2::{Digest as ShaDigest, Sha256};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(12);
const CODE_LIFETIME_MINUTES: i64 = 10;

fn token(state: &App) -> Result<&str, AppError> {
    state.config.telegram_bot_token.as_deref().filter(|v| !v.trim().is_empty()).ok_or_else(|| AppError::Unavailable("Telegram is not configured".into()))
}

fn api_url(token: &str, method: &str) -> String { format!("https://api.telegram.org/bot{token}/{method}") }

async fn call(state: &App, method: &str, body: Value) -> Result<Value, AppError> {
    let token = token(state)?;
    let client = reqwest::Client::builder().timeout(TIMEOUT).build().map_err(|e| AppError::Internal(format!("telegram client: {e}")))?;
    let response = client.post(api_url(token, method)).json(&body).send().await.map_err(|e| AppError::Internal(format!("telegram request: {}", e.without_url())))?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|e| AppError::Internal(format!("telegram response: {e}")))?;
    if !status.is_success() || value.get("ok") != Some(&Value::Bool(true)) {
        return Err(AppError::Internal(format!("telegram API returned {}", status)));
    }
    Ok(value)
}

fn hash(code: &str) -> String { hex::encode(Sha256::digest(code.as_bytes())) }

pub fn configured(state: &App) -> bool { state.config.telegram_bot_token.as_deref().is_some_and(|v| !v.trim().is_empty()) }

pub async fn create_link(state: &App, user_id: i64) -> Result<String, AppError> {
    token(state)?;
    let me = call(state, "getMe", json!({})).await?;
    let username = me["result"]["username"].as_str().ok_or_else(|| AppError::Internal("Telegram bot has no username".into()))?;
    let mut bytes = [0u8; 18]; rand::rng().fill(&mut bytes);
    let code = hex::encode(bytes);
    let expires = (Utc::now() + ChronoDuration::minutes(CODE_LIFETIME_MINUTES)).to_rfc3339();
    sqlx::query("DELETE FROM telegram_link_codes WHERE user_id = $1 OR expires_at <= $2")
        .bind(user_id).bind(db::now()).execute(&state.db).await?;
    sqlx::query("INSERT INTO telegram_link_codes (user_id, code_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(user_id).bind(hash(&code)).bind(&expires).execute(&state.db).await?;
    Ok(format!("https://t.me/{username}?start={code}"))
}

pub async fn unlink(state: &App, user_id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM telegram_connections WHERE user_id = $1").bind(user_id).execute(&state.db).await?;
    Ok(())
}

pub async fn status(state: &App, user_id: i64) -> Result<Option<(String, Option<String>)>, AppError> {
    Ok(sqlx::query_as("SELECT display_name, last_error FROM telegram_connections WHERE user_id = $1 AND disabled_at IS NULL")
        .bind(user_id).fetch_optional(&state.db).await?)
}

pub async fn send(state: &App, chat_id: &str, digest: &Digest) -> Result<(), AppError> {
    call(state, "sendMessage", json!({"chat_id": chat_id, "text": format!("{}\n\n{}", digest.title, digest.message), "disable_web_page_preview": false})).await.map(|_| ())
}

pub async fn poll(state: &App) -> Result<(), AppError> {
    if !configured(state) { return Ok(()); }
    let raw: String = sqlx::query_scalar("SELECT COALESCE((SELECT value FROM settings WHERE key = 'telegram_update_offset'), '0')")
        .fetch_one(&state.db).await?;
    let offset: i64 = raw.parse().unwrap_or(0);
    let result = call(state, "getUpdates", json!({"offset": offset, "timeout": 1, "allowed_updates": ["message"]})).await?;
    let updates = result["result"].as_array().cloned().unwrap_or_default();
    for update in updates {
        let id = update["update_id"].as_i64().unwrap_or(0);
        if id >= offset { sqlx::query("INSERT INTO settings (key,value) VALUES ('telegram_update_offset',$1) ON CONFLICT (key) DO UPDATE SET value=excluded.value").bind((id + 1).to_string()).execute(&state.db).await?; }
        let Some(text) = update["message"]["text"].as_str() else { continue };
        let Some(code) = text.strip_prefix("/start ").map(str::trim) else { continue };
        if code.is_empty() { continue; }
        let Some(user_id): Option<i64> = sqlx::query_scalar("SELECT user_id FROM telegram_link_codes WHERE code_hash = $1 AND used_at IS NULL AND expires_at > $2")
            .bind(hash(code)).bind(db::now()).fetch_optional(&state.db).await? else { continue; };
        let chat_id = update["message"]["chat"]["id"].to_string();
        let name = update["message"]["from"]["first_name"].as_str().unwrap_or("Telegram").to_string();
        sqlx::query("DELETE FROM telegram_connections WHERE user_id = $1 OR chat_id = $2").bind(user_id).bind(&chat_id).execute(&state.db).await?;
        sqlx::query("INSERT INTO telegram_connections (user_id,chat_id,display_name,created_at,updated_at) VALUES ($1,$2,$3,$4,$4)")
            .bind(user_id).bind(&chat_id).bind(&name).bind(db::now()).execute(&state.db).await?;
        sqlx::query("UPDATE telegram_link_codes SET used_at = $1 WHERE user_id = $2 AND code_hash = $3").bind(db::now()).bind(user_id).bind(hash(code)).execute(&state.db).await?;
        let _ = send(state, &chat_id, &Digest { title: "LogB connected".into(), message: "Telegram notifications are now enabled.".into(), reminders: vec![] }).await;
    }
    Ok(())
}

pub async fn send_user(state: &App, user_id: i64, digest: &Digest) -> Result<(), AppError> {
    let Some((chat, _)) = status(state, user_id).await? else { return Ok(()); };
    send(state, &chat, digest).await
}
