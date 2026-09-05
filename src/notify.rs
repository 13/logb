//! Daily digest of due reminders, pushed to a webhook.
//!
//! memto has no mail transport and no push infrastructure of its own: a self-hosted instance
//! posts to a URL the operator already runs -- an ntfy topic, a chat webhook, a home-automation
//! endpoint -- and lets that decide how the reminder reaches a phone.

use crate::api::reminders::due_for_user;
use crate::db;
use crate::error::AppError;
use crate::state::App;
use serde::Serialize;
use std::time::Duration;

/// Key in the `settings` table holding the date (`YYYY-MM-DD`) of the last digest attempt.
const LAST_SENT_KEY: &str = "notify_last_sent";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
/// How often the scheduler wakes to ask whether today's digest is due yet.
const TICK: Duration = Duration::from_secs(60);

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct DueItem {
    pub username: String,
    pub object_id: i64,
    pub object_name: String,
    pub reminder_id: i64,
    pub title: String,
    pub due_date: Option<String>,
    pub due_counter: Option<i64>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Digest {
    pub title: String,
    pub message: String,
    pub reminders: Vec<DueItem>,
}

/// Every due reminder across every user, or `None` when nothing is due.
///
/// Deliberately one digest for the whole instance rather than one per user: the webhook is
/// the operator's, not each user's, so splitting it would send someone else's reminders to
/// the same endpoint anyway, just in more requests.
pub async fn collect(state: &App) -> Result<Option<Digest>, AppError> {
    let users: Vec<(i64, String)> = sqlx::query_as("SELECT id, username FROM users ORDER BY username")
        .fetch_all(&state.db).await?;
    let mut items = Vec::new();
    for (user_id, username) in users {
        for r in due_for_user(state, user_id).await? {
            items.push(DueItem {
                username: username.clone(),
                object_id: r.row.object_id,
                object_name: r.row.object_name,
                reminder_id: r.row.id,
                title: r.row.title,
                due_date: r.row.due_date,
                due_counter: r.row.due_counter,
            });
        }
    }
    if items.is_empty() {
        return Ok(None);
    }
    let title = if items.len() == 1 {
        "memto: 1 reminder due".to_string()
    } else {
        format!("memto: {} reminders due", items.len())
    };
    let message = items
        .iter()
        .map(|i| format!("{}: {}", i.object_name, i.title))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(Digest { title, message, reminders: items }))
}

/// POSTs the digest to the configured URL. `text` sends the plain message with the summary in
/// a `Title` header, which is what ntfy-style services render; anything else sends JSON.
pub async fn send(state: &App, digest: &Digest) -> Result<(), AppError> {
    let Some(url) = state.config.notify_url.as_deref() else { return Ok(()) };
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| AppError::Internal(format!("notify client: {e}")))?;
    let request = if state.config.notify_format == "text" {
        client.post(url).header("Title", &digest.title).body(digest.message.clone())
    } else {
        client.post(url).json(digest)
    };
    let res = request.send().await.map_err(|e| AppError::Internal(format!("notify post: {e}")))?;
    if !res.status().is_success() {
        return Err(AppError::Internal(format!("notify endpoint returned {}", res.status())));
    }
    Ok(())
}

async fn last_sent(state: &App) -> Result<Option<String>, AppError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(LAST_SENT_KEY).fetch_optional(&state.db).await?;
    Ok(row.map(|r| r.0))
}

async fn mark_sent(state: &App, date: &str) -> Result<(), AppError> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
        .bind(LAST_SENT_KEY).bind(date).execute(&state.db).await?;
    Ok(())
}

/// One scheduler tick. Returns the digest it sent, if any.
///
/// The day is marked as handled *before* the POST goes out, so an endpoint that is down or
/// misconfigured costs one failed request per day rather than one per minute until midnight.
/// The trade is that a digest lost to a transient failure is not retried; the reminders stay
/// due and appear in tomorrow's.
pub async fn tick(state: &App, hour_now: u32) -> Result<Option<Digest>, AppError> {
    if state.config.notify_url.is_none() || hour_now < state.config.notify_hour {
        return Ok(None);
    }
    let today = db::today();
    if last_sent(state).await?.as_deref() == Some(today.as_str()) {
        return Ok(None);
    }
    mark_sent(state, &today).await?;
    let Some(digest) = collect(state).await? else { return Ok(None) };
    send(state, &digest).await?;
    Ok(Some(digest))
}

/// Starts the daily digest scheduler. A no-op when no notify URL is configured.
pub fn spawn(state: App) {
    if state.config.notify_url.is_none() {
        return;
    }
    tracing::info!(hour = state.config.notify_hour, "reminder digest enabled");
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            let hour = chrono::Utc::now().format("%H").to_string().parse().unwrap_or(0);
            match tick(&state, hour).await {
                Ok(Some(d)) => tracing::info!(reminders = d.reminders.len(), "sent reminder digest"),
                Ok(None) => {}
                Err(e) => tracing::warn!(error = %e, "reminder digest failed"),
            }
        }
    });
}
