//! Telegram Bot API integration. The instance owns one bot; a signed-in user links one private
//! chat with a short-lived, one-use code.

use crate::{db, error::AppError, notify::Digest, state::App};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rand::RngExt;
use serde_json::{json, Value};
use sha2::{Digest as ShaDigest, Sha256};
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(35);
const CODE_LIFETIME_MINUTES: i64 = 10;
const MAX_MESSAGE_CHARS: usize = 4096;
const POLL_BACKOFF: Duration = Duration::from_secs(5);

fn token(state: &App) -> Result<&str, AppError> {
    state.config.telegram_bot_token.as_deref().filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::Unavailable("Telegram is not configured".into()))
}

fn api_url(state: &App, token: &str, method: &str) -> String {
    format!("{}/bot{token}/{method}", state.config.telegram_api_url.trim_end_matches('/'))
}

fn telegram_error(status: reqwest::StatusCode, value: &Value) -> AppError {
    let code = value["error_code"].as_i64().unwrap_or(status.as_u16() as i64);
    let description = value["description"].as_str().unwrap_or("Telegram request failed");
    AppError::Internal(format!("telegram {code}: {description}"))
}

async fn call(state: &App, method: &str, body: &Value) -> Result<Value, AppError> {
    let token = token(state)?;
    let client = reqwest::Client::builder().timeout(HTTP_TIMEOUT).build()
        .map_err(|e| AppError::Internal(format!("telegram client: {e}")))?;
    for attempt in 0..2 {
        let response = client.post(api_url(state, token, method)).json(body).send().await
            .map_err(|e| AppError::Internal(format!("telegram request: {}", e.without_url())))?;
        let status = response.status();
        let value: Value = response.json().await
            .map_err(|e| AppError::Internal(format!("telegram response: {e}")))?;
        if status.is_success() && value.get("ok") == Some(&Value::Bool(true)) { return Ok(value); }
        let retry = value["parameters"]["retry_after"].as_u64();
        if let Some(seconds) = retry.filter(|_| status.as_u16() == 429 && attempt == 0) {
            tokio::time::sleep(Duration::from_secs(seconds.min(30))).await;
            continue;
        }
        return Err(telegram_error(status, &value));
    }
    unreachable!()
}

fn hash(code: &str) -> String { hex::encode(Sha256::digest(code.as_bytes())) }

pub fn configured(state: &App) -> bool {
    state.config.telegram_bot_token.as_deref().is_some_and(|v| !v.trim().is_empty())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status { pub display_name: String, pub last_error: Option<String>, pub connected: bool }

pub async fn status(state: &App, user_id: i64) -> Result<Option<Status>, AppError> {
    let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT display_name, last_error, disabled_at FROM telegram_connections WHERE user_id = $1")
        .bind(user_id).fetch_optional(&state.db).await?;
    Ok(row.map(|(display_name, last_error, disabled_at)| Status { display_name, last_error, connected: disabled_at.is_none() }))
}

pub async fn connected(state: &App, user_id: i64) -> Result<bool, AppError> {
    Ok(status(state, user_id).await?.is_some_and(|s| s.connected))
}

pub struct Link { pub url: String, pub qr_svg: String, pub expires_at: String }

pub async fn create_link(state: &App, user_id: i64) -> Result<Link, AppError> {
    token(state)?;
    let me = call(state, "getMe", &json!({})).await?;
    let username = me["result"]["username"].as_str()
        .ok_or_else(|| AppError::Internal("Telegram bot has no username".into()))?;
    let mut bytes = [0u8; 18]; rand::rng().fill(&mut bytes);
    let code = hex::encode(bytes);
    let expires_at = (Utc::now() + ChronoDuration::minutes(CODE_LIFETIME_MINUTES)).to_rfc3339();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("DELETE FROM telegram_link_codes WHERE user_id = $1")
        .bind(user_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO telegram_link_codes (user_id, code_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(user_id).bind(hash(&code)).bind(&expires_at).execute(&mut *tx).await?;
    tx.commit().await?;
    let url = format!("https://t.me/{username}?start={code}");
    Ok(Link { qr_svg: crate::domain::pairing::qr_svg(&url), url, expires_at })
}

pub async fn unlink(state: &App, user_id: i64) -> Result<(), AppError> {
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("DELETE FROM telegram_connections WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM telegram_link_codes WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

fn message_chunks(digest: &Digest) -> Vec<String> {
    let text = format!("{}\n\n{}", digest.title, digest.message);
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() == MAX_MESSAGE_CHARS { chunks.push(std::mem::take(&mut current)); }
        current.push(ch);
    }
    if !current.is_empty() { chunks.push(current); }
    chunks
}

async fn send_chat(state: &App, chat_id: &str, digest: &Digest) -> Result<(), AppError> {
    for text in message_chunks(digest) {
        call(state, "sendMessage", &json!({"chat_id": chat_id, "text": text, "disable_web_page_preview": false})).await?;
    }
    Ok(())
}

pub async fn send_user(state: &App, user_id: i64, digest: &Digest) -> Result<(), AppError> {
    let chat: Option<String> = sqlx::query_scalar(
        "SELECT chat_id FROM telegram_connections WHERE user_id = $1 AND disabled_at IS NULL")
        .bind(user_id).fetch_optional(&state.db).await?;
    let Some(chat_id) = chat else { return Ok(()) };
    match send_chat(state, &chat_id, digest).await {
        Ok(()) => {
            sqlx::query("UPDATE telegram_connections SET last_error = NULL, updated_at = $1 WHERE user_id = $2")
                .bind(db::now()).bind(user_id).execute(&state.db).await?;
            Ok(())
        }
        Err(e) => {
            let reason = e.to_string();
            let disabled = reason.starts_with("telegram 403:");
            sqlx::query("UPDATE telegram_connections SET last_error = $1, disabled_at = CASE WHEN $2 THEN $3 ELSE disabled_at END, updated_at = $3 WHERE user_id = $4")
                .bind(&reason).bind(disabled).bind(db::now()).bind(user_id).execute(&state.db).await?;
            Err(e)
        }
    }
}

async fn advance_offset(state: &App, next: i64) -> Result<(), AppError> {
    sqlx::query("INSERT INTO settings (key,value) VALUES ('telegram_update_offset',$1) ON CONFLICT (key) DO UPDATE SET value=excluded.value")
        .bind(next.to_string()).execute(&state.db).await?;
    Ok(())
}

async fn link_update(state: &App, update: &Value, update_id: i64, code: &str) -> Result<bool, AppError> {
    if update["message"]["chat"]["type"].as_str() != Some("private") { return Ok(false); }
    let Some(chat_number) = update["message"]["chat"]["id"].as_i64() else { return Ok(false) };
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let code_row: Option<(i64, String)> = sqlx::query_as(
        "SELECT user_id, expires_at FROM telegram_link_codes WHERE code_hash = $1 AND used_at IS NULL")
        .bind(hash(code)).fetch_optional(&mut *tx).await?;
    let Some((user_id, expires_at)) = code_row else { tx.rollback().await?; return Ok(false) };
    let live = DateTime::parse_from_rfc3339(&expires_at).map(|v| v.with_timezone(&Utc) > Utc::now()).unwrap_or(false);
    if !live { tx.rollback().await?; return Ok(false); }
    let chat_id = chat_number.to_string();
    let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM telegram_connections WHERE chat_id = $1")
        .bind(&chat_id).fetch_optional(&mut *tx).await?;
    if owner.is_some_and(|owner| owner != user_id) { tx.rollback().await?; return Ok(false); }
    let name = update["message"]["from"]["first_name"].as_str().unwrap_or("Telegram");
    sqlx::query("DELETE FROM telegram_connections WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO telegram_connections (user_id,chat_id,display_name,created_at,updated_at) VALUES ($1,$2,$3,$4,$4)")
        .bind(user_id).bind(&chat_id).bind(name).bind(db::now()).execute(&mut *tx).await?;
    sqlx::query("UPDATE telegram_link_codes SET used_at = $1 WHERE user_id = $2 AND code_hash = $3")
        .bind(db::now()).bind(user_id).bind(hash(code)).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO settings (key,value) VALUES ('telegram_update_offset',$1) ON CONFLICT (key) DO UPDATE SET value=excluded.value")
        .bind((update_id + 1).to_string()).execute(&mut *tx).await?;
    tx.commit().await?;
    let welcome = Digest { title: "LogB connected".into(), message: "Telegram notifications are now enabled.".into(), reminders: vec![] };
    let _ = send_chat(state, &chat_id, &welcome).await;
    Ok(true)
}

pub async fn poll_once(state: &App) -> Result<(), AppError> {
    if !configured(state) { return Ok(()) }
    let raw: String = sqlx::query_scalar("SELECT COALESCE((SELECT value FROM settings WHERE key = 'telegram_update_offset'), '0')")
        .fetch_one(&state.db).await?;
    let offset = raw.parse::<i64>().unwrap_or(0);
    let result = call(state, "getUpdates", &json!({"offset": offset, "timeout": 20, "allowed_updates": ["message"]})).await?;
    for update in result["result"].as_array().cloned().unwrap_or_default() {
        let Some(id) = update["update_id"].as_i64() else { continue };
        let code = update["message"]["text"].as_str().and_then(|v| v.strip_prefix("/start ")).map(str::trim);
        if let Some(code) = code.filter(|v| !v.is_empty()) {
            if link_update(state, &update, id, code).await? { continue; }
        }
        advance_offset(state, id + 1).await?;
    }
    Ok(())
}

pub fn spawn(state: App) {
    if !configured(&state) { return }
    tokio::spawn(async move {
        loop {
            match poll_once(&state).await {
                Ok(()) => tokio::time::sleep(Duration::from_millis(250)).await,
                Err(e) => {
                    tracing::warn!(error = %e, "telegram polling failed");
                    tokio::time::sleep(POLL_BACKOFF).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_messages_are_split_at_telegram_limit() {
        let d = Digest { title: "T".into(), message: "x".repeat(9000), reminders: vec![] };
        let chunks = message_chunks(&d);
        assert_eq!(chunks.concat(), format!("T\n\n{}", "x".repeat(9000)));
        assert!(chunks.iter().all(|c| c.chars().count() <= MAX_MESSAGE_CHARS));
        assert_eq!(chunks.len(), 3);
    }
}
