//! Telegram Bot API integration. Each user owns a bot token and links one private chat.
use crate::{db, error::AppError, notify::Digest, state::App};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rand::RngExt;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::{fs::OpenOptions, io::Write, path::PathBuf, time::Duration};

const HTTP_TIMEOUT: Duration = Duration::from_secs(35);
const CODE_LIFETIME_MINUTES: i64 = 10;
const MAX_MESSAGE_CHARS: usize = 4096;

fn legacy_token(state: &App) -> Option<&str> {
    state
        .config
        .telegram_bot_token
        .as_deref()
        .filter(|v| !v.trim().is_empty())
}
fn hash(v: &str) -> String {
    hex::encode(Sha256::digest(v.as_bytes()))
}
fn key_path(state: &App) -> PathBuf {
    state.config.data_dir.join("telegram.key")
}
fn api_url(state: &App, token: &str, method: &str) -> String {
    format!(
        "{}/bot{token}/{method}",
        state.config.telegram_api_url.trim_end_matches('/')
    )
}
fn telegram_error(status: reqwest::StatusCode, value: &Value) -> AppError {
    AppError::Internal(format!(
        "telegram {}: {}",
        value["error_code"]
            .as_i64()
            .unwrap_or(status.as_u16() as i64),
        value["description"].as_str().unwrap_or("request failed")
    ))
}
async fn call(state: &App, token: &str, method: &str, body: &Value) -> Result<Value, AppError> {
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| AppError::Internal(format!("telegram client: {e}")))?;
    for attempt in 0..2 {
        let response = client
            .post(api_url(state, token, method))
            .json(body)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("telegram request: {}", e.without_url())))?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("telegram response: {e}")))?;
        if status.is_success() && value["ok"] == true {
            return Ok(value);
        }
        if let Some(seconds) = value["parameters"]["retry_after"]
            .as_u64()
            .filter(|_| status.as_u16() == 429 && attempt == 0)
        {
            tokio::time::sleep(Duration::from_secs(seconds.min(30))).await;
            continue;
        }
        return Err(telegram_error(status, &value));
    }
    unreachable!()
}
fn read_key(state: &App) -> Result<[u8; 32], AppError> {
    std::fs::read(key_path(state))
        .map_err(|_| {
            AppError::Unavailable(
                "Telegram's encryption key is missing; save the bot token again".into(),
            )
        })?
        .try_into()
        .map_err(|_| AppError::Internal("Telegram encryption key has the wrong size".into()))
}
fn save_key(state: &App) -> Result<[u8; 32], AppError> {
    if key_path(state).exists() {
        return read_key(state);
    }
    let mut key = [0u8; 32];
    rand::rng().fill(&mut key);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(key_path(state)) {
        Ok(mut f) => {
            f.write_all(&key)?;
            f.sync_all()?;
            Ok(key)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => read_key(state),
        Err(e) => Err(e.into()),
    }
}
fn encrypt(state: &App, token: &str) -> Result<(String, String), AppError> {
    let cipher = Aes256Gcm::new_from_slice(&save_key(state)?)
        .map_err(|_| AppError::Internal("invalid Telegram key".into()))?;
    let mut nonce = [0u8; 12];
    rand::rng().fill(&mut nonce);
    let data = cipher
        .encrypt(&Nonce::from(nonce), token.as_bytes())
        .map_err(|_| AppError::Internal("could not encrypt Telegram token".into()))?;
    Ok((hex::encode(data), hex::encode(nonce)))
}
fn decrypt(state: &App, data: &str, nonce: &str) -> Result<String, AppError> {
    let data = hex::decode(data)
        .map_err(|_| AppError::Internal("invalid encrypted Telegram token".into()))?;
    let nonce =
        hex::decode(nonce).map_err(|_| AppError::Internal("invalid Telegram nonce".into()))?;
    let nonce: [u8; 12] = nonce
        .try_into()
        .map_err(|_| AppError::Internal("invalid Telegram nonce".into()))?;
    let cipher = Aes256Gcm::new_from_slice(&read_key(state)?)
        .map_err(|_| AppError::Internal("invalid Telegram key".into()))?;
    let plain = cipher
        .decrypt(&Nonce::from(nonce), data.as_ref())
        .map_err(|_| {
            AppError::Unavailable(
                "Telegram credentials cannot be decrypted; save the bot token again".into(),
            )
        })?;
    String::from_utf8(plain).map_err(|_| AppError::Internal("invalid Telegram credential".into()))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub configured: bool,
    pub bot_username: Option<String>,
    pub display_name: Option<String>,
    pub last_error: Option<String>,
    pub connected: bool,
    pub legacy: bool,
}
pub async fn status(state: &App, user_id: i64) -> Result<Status, AppError> {
    let credential: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT bot_username,last_error FROM telegram_credentials WHERE user_id=$1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    let connection: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT display_name,last_error,disabled_at FROM telegram_connections WHERE user_id=$1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    let legacy = credential.is_none() && connection.is_some() && legacy_token(state).is_some();
    Ok(Status {
        configured: credential.is_some() || legacy,
        bot_username: credential.as_ref().map(|v| v.0.clone()),
        display_name: connection.as_ref().map(|v| v.0.clone()),
        last_error: connection
            .as_ref()
            .and_then(|v| v.1.clone())
            .or_else(|| credential.as_ref().and_then(|v| v.1.clone())),
        connected: connection.as_ref().is_some_and(|v| v.2.is_none())
            && (credential.is_some() || legacy),
        legacy,
    })
}
pub async fn connected(state: &App, user_id: i64) -> Result<bool, AppError> {
    Ok(status(state, user_id).await?.connected)
}

pub async fn save_token(state: &App, user_id: i64, raw: &str) -> Result<(), AppError> {
    let token = raw.trim();
    if token.is_empty() || token.len() > 256 || !token.contains(':') {
        return Err(AppError::BadRequest(
            "enter a valid Telegram bot token".into(),
        ));
    }
    let me = call(state, token, "getMe", &json!({}))
        .await
        .map_err(|_| AppError::BadRequest("Telegram rejected this bot token".into()))?;
    let username = me["result"]["username"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("this Telegram bot has no username".into()))?;
    let (cipher, nonce) = encrypt(state, token)?;
    let now = db::now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("DELETE FROM telegram_connections WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM telegram_link_codes WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("INSERT INTO telegram_credentials (user_id,token_cipher,token_nonce,token_hash,bot_username,created_at,updated_at) VALUES ($1,$2,$3,$4,$5,$6,$6) ON CONFLICT (user_id) DO UPDATE SET token_cipher=excluded.token_cipher,token_nonce=excluded.token_nonce,token_hash=excluded.token_hash,bot_username=excluded.bot_username,update_offset='0',last_error=NULL,updated_at=excluded.updated_at")
        .bind(user_id).bind(cipher).bind(nonce).bind(hash(token)).bind(username).bind(now).execute(&mut *tx).await;
    if let Err(e) = result {
        if e.as_database_error()
            .is_some_and(|v| v.is_unique_violation())
        {
            return Err(AppError::Conflict(
                "this Telegram bot is already used by another account".into(),
            ));
        }
        return Err(e.into());
    }
    tx.commit().await?;
    Ok(())
}
pub async fn remove_token(state: &App, user_id: i64) -> Result<(), AppError> {
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("DELETE FROM telegram_connections WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM telegram_link_codes WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM telegram_credentials WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
async fn user_token(state: &App, user_id: i64) -> Result<String, AppError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT token_cipher,token_nonce FROM telegram_credentials WHERE user_id=$1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some((data, nonce)) => decrypt(state, &data, &nonce),
        None => legacy_token(state)
            .map(str::to_owned)
            .ok_or_else(|| AppError::Unavailable("save a Telegram bot token first".into())),
    }
}

pub struct Link {
    pub url: String,
    pub qr_svg: String,
    pub expires_at: String,
}
pub async fn create_link(state: &App, user_id: i64) -> Result<Link, AppError> {
    let token = user_token(state, user_id).await?;
    let me = call(state, &token, "getMe", &json!({})).await?;
    let username = me["result"]["username"]
        .as_str()
        .ok_or_else(|| AppError::Internal("Telegram bot has no username".into()))?;
    let mut bytes = [0u8; 18];
    rand::rng().fill(&mut bytes);
    let code = hex::encode(bytes);
    let expires_at = (Utc::now() + ChronoDuration::minutes(CODE_LIFETIME_MINUTES)).to_rfc3339();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("DELETE FROM telegram_link_codes WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO telegram_link_codes (user_id,code_hash,expires_at) VALUES ($1,$2,$3)")
        .bind(user_id)
        .bind(hash(&code))
        .bind(&expires_at)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let url = format!("https://t.me/{username}?start={code}");
    Ok(Link {
        qr_svg: crate::domain::pairing::qr_svg(&url),
        url,
        expires_at,
    })
}
pub async fn unlink(state: &App, user_id: i64) -> Result<(), AppError> {
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("DELETE FROM telegram_connections WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM telegram_link_codes WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
fn message_chunks(digest: &Digest) -> Vec<String> {
    let text = format!("{}\n\n{}", digest.title, digest.message);
    let mut chunks = vec![];
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() == MAX_MESSAGE_CHARS {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}
async fn send_chat(
    state: &App,
    token: &str,
    chat_id: &str,
    digest: &Digest,
) -> Result<(), AppError> {
    for text in message_chunks(digest) {
        call(
            state,
            token,
            "sendMessage",
            &json!({"chat_id":chat_id,"text":text,"disable_web_page_preview":false}),
        )
        .await?;
    }
    Ok(())
}
pub async fn send_user(state: &App, user_id: i64, digest: &Digest) -> Result<(), AppError> {
    let chat: Option<String> = sqlx::query_scalar(
        "SELECT chat_id FROM telegram_connections WHERE user_id=$1 AND disabled_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    let Some(chat_id) = chat else { return Ok(()) };
    let token = user_token(state, user_id).await?;
    match send_chat(state, &token, &chat_id, digest).await {
        Ok(()) => {
            sqlx::query(
                "UPDATE telegram_connections SET last_error=NULL,updated_at=$1 WHERE user_id=$2",
            )
            .bind(db::now())
            .bind(user_id)
            .execute(&state.db)
            .await?;
            Ok(())
        }
        Err(e) => {
            let reason = e.to_string();
            let disabled = reason.starts_with("telegram 403:");
            sqlx::query("UPDATE telegram_connections SET last_error=$1,disabled_at=CASE WHEN $2 THEN $3 ELSE disabled_at END,updated_at=$3 WHERE user_id=$4").bind(&reason).bind(disabled).bind(db::now()).bind(user_id).execute(&state.db).await?;
            Err(e)
        }
    }
}
async fn link_update(
    state: &App,
    expected: Option<i64>,
    token: &str,
    update: &Value,
    code: &str,
) -> Result<bool, AppError> {
    if update["message"]["chat"]["type"] != "private" {
        return Ok(false);
    }
    let Some(chat_number) = update["message"]["chat"]["id"].as_i64() else {
        return Ok(false);
    };
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT user_id,expires_at FROM telegram_link_codes WHERE code_hash=$1 AND used_at IS NULL",
    )
    .bind(hash(code))
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_id, expires_at)) = row else {
        tx.rollback().await?;
        return Ok(false);
    };
    if expected.is_some_and(|id| id != user_id)
        || !DateTime::parse_from_rfc3339(&expires_at)
            .is_ok_and(|v| v.with_timezone(&Utc) > Utc::now())
    {
        tx.rollback().await?;
        return Ok(false);
    }
    let chat_id = chat_number.to_string();
    let owner: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM telegram_connections WHERE chat_id=$1")
            .bind(&chat_id)
            .fetch_optional(&mut *tx)
            .await?;
    if owner.is_some_and(|id| id != user_id) {
        tx.rollback().await?;
        return Ok(false);
    }
    let name = update["message"]["from"]["first_name"]
        .as_str()
        .unwrap_or("Telegram");
    sqlx::query("DELETE FROM telegram_connections WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO telegram_connections (user_id,chat_id,display_name,created_at,updated_at) VALUES ($1,$2,$3,$4,$4)").bind(user_id).bind(&chat_id).bind(name).bind(db::now()).execute(&mut *tx).await?;
    sqlx::query("UPDATE telegram_link_codes SET used_at=$1 WHERE user_id=$2 AND code_hash=$3")
        .bind(db::now())
        .bind(user_id)
        .bind(hash(code))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let welcome = Digest {
        title: "LogB connected".into(),
        message: "Telegram notifications are now enabled.".into(),
        reminders: vec![],
    };
    let _ = send_chat(state, token, &chat_id, &welcome).await;
    Ok(true)
}
async fn poll_bot(
    state: &App,
    user_id: Option<i64>,
    token: &str,
    offset: i64,
) -> Result<i64, AppError> {
    let result = call(
        state,
        token,
        "getUpdates",
        &json!({"offset":offset,"timeout":0,"allowed_updates":["message"]}),
    )
    .await?;
    let mut next = offset;
    for update in result["result"].as_array().cloned().unwrap_or_default() {
        let Some(id) = update["update_id"].as_i64() else {
            continue;
        };
        if let Some(code) = update["message"]["text"]
            .as_str()
            .and_then(|v| v.strip_prefix("/start "))
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let _ = link_update(state, user_id, token, &update, code).await?;
        }
        next = next.max(id + 1);
    }
    Ok(next)
}
pub async fn poll_once(state: &App) -> Result<(), AppError> {
    let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT user_id,token_cipher,token_nonce,update_offset FROM telegram_credentials",
    )
    .fetch_all(&state.db)
    .await?;
    for (user_id, data, nonce, raw) in rows {
        let token = decrypt(state, &data, &nonce)?;
        let old = raw.parse().unwrap_or(0);
        let next = poll_bot(state, Some(user_id), &token, old).await?;
        if next != old {
            sqlx::query(
                "UPDATE telegram_credentials SET update_offset=$1,updated_at=$2 WHERE user_id=$3",
            )
            .bind(next.to_string())
            .bind(db::now())
            .bind(user_id)
            .execute(&state.db)
            .await?;
        }
    }
    if let Some(token) = legacy_token(state) {
        let raw: String = sqlx::query_scalar(
            "SELECT COALESCE((SELECT value FROM settings WHERE key='telegram_update_offset'),'0')",
        )
        .fetch_one(&state.db)
        .await?;
        let old = raw.parse().unwrap_or(0);
        let next = poll_bot(state, None, token, old).await?;
        if next != old {
            sqlx::query("INSERT INTO settings (key,value) VALUES ('telegram_update_offset',$1) ON CONFLICT (key) DO UPDATE SET value=excluded.value").bind(next.to_string()).execute(&state.db).await?;
        }
    }
    Ok(())
}
pub fn spawn(state: App) {
    tokio::spawn(async move {
        loop {
            match poll_once(&state).await {
                Ok(()) => tokio::time::sleep(Duration::from_secs(2)).await,
                Err(e) => {
                    tracing::warn!(error=%e,"telegram polling failed");
                    tokio::time::sleep(Duration::from_secs(5)).await
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
        let d = Digest {
            title: "T".into(),
            message: "x".repeat(9000),
            reminders: vec![],
        };
        let chunks = message_chunks(&d);
        assert_eq!(chunks.concat(), format!("T\n\n{}", "x".repeat(9000)));
        assert!(chunks
            .iter()
            .all(|c| c.chars().count() <= MAX_MESSAGE_CHARS));
        assert_eq!(chunks.len(), 3);
    }
}
