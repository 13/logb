//! Daily digest of due reminders, pushed to a webhook.
//!
//! LogB has no mail transport and no push infrastructure of its own: a self-hosted instance
//! posts to a URL the operator already runs -- an ntfy topic, a chat webhook, a home-automation
//! endpoint -- and lets that decide how the reminder reaches a phone.

use crate::api::reminders::due_for_user;
use crate::db;
use crate::domain::reminder::KIND_READING;
use crate::error::AppError;
use crate::state::App;
use serde::Serialize;
use std::time::Duration;

/// Key in the `settings` table holding the date (`YYYY-MM-DD`) of the last digest attempt.
const LAST_SENT_KEY: &str = "notify_last_sent";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct DueItem {
    pub username: String,
    pub object_id: i64,
    pub object_name: String,
    pub reminder_id: i64,
    pub title: String,
    pub due_date: Option<String>,
    pub due_counter: Option<i64>,
    /// `service` or `reading`.
    pub kind: String,
    /// Where in the app to act on it, when `LOGB_PUBLIC_URL` says where the app is: the quick
    /// reading form for a reading, the object's reminders for anything else.
    pub link: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Digest {
    pub title: String,
    pub message: String,
    pub reminders: Vec<DueItem>,
}

/// The heading the digest's reading section starts with.
const READINGS_HEADING: &str = "Readings to log:";

fn link(public_url: Option<&str>, object_id: i64, kind: &str) -> Option<String> {
    let base = public_url?.trim_end_matches('/');
    Some(if kind == KIND_READING {
        format!("{base}/objects/{object_id}/reading")
    } else {
        format!("{base}/objects/{object_id}?tab=reminders")
    })
}

/// Every due reminder across every user, or `None` when nothing is due.
///
/// Deliberately one digest for the whole instance rather than one per user: the webhook is
/// the operator's, not each user's, so splitting it would send someone else's reminders to
/// the same endpoint anyway, just in more requests.
pub async fn collect(state: &App) -> Result<Option<Digest>, AppError> {
    let users: Vec<(i64, String)> = sqlx::query_as("SELECT id, username FROM users ORDER BY username")
        .fetch_all(&state.db).await?;
    let public_url = state.config.public_url.as_deref();
    let mut items = Vec::new();
    for (user_id, username) in users {
        for r in due_for_user(state, user_id, 0).await? {
            items.push(DueItem {
                username: username.clone(),
                object_id: r.row.object_id,
                link: link(public_url, r.row.object_id, &r.row.kind),
                object_name: r.row.object_name,
                reminder_id: r.row.id,
                title: r.row.title,
                due_date: r.next_due_date,
                due_counter: r.row.due_counter,
                kind: r.row.kind,
            });
        }
    }
    if items.is_empty() {
        return Ok(None);
    }
    let title = if items.len() == 1 {
        "LogB: 1 reminder due".to_string()
    } else {
        format!("LogB: {} reminders due", items.len())
    };

    // Services first, as they always were; readings below under a heading of their own, because
    // "log the odometer" is a thirty-second job and reads differently next to "brakes are due".
    // A reading's line carries its link, so a phone notification opens straight into the form.
    let services: Vec<String> = items
        .iter()
        .filter(|i| i.kind != KIND_READING)
        .map(|i| format!("{}: {}", i.object_name, i.title))
        .collect();
    let readings: Vec<String> = items
        .iter()
        .filter(|i| i.kind == KIND_READING)
        .map(|i| match &i.link {
            Some(url) => format!("{}: {} {url}", i.object_name, i.title),
            None => format!("{}: {}", i.object_name, i.title),
        })
        .collect();
    let mut message = services.join("\n");
    if !readings.is_empty() {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str(READINGS_HEADING);
        message.push('\n');
        message.push_str(&readings.join("\n"));
    }
    Ok(Some(Digest { title, message, reminders: items }))
}

/// POSTs the digest to the configured URL. `text` sends the plain message with the summary in
/// a `Title` header, which is what ntfy-style services render; anything else sends JSON.
///
/// A text digest about exactly one reminder also sends a `Click` header naming its link, so
/// tapping the notification opens the app where that reminder is dealt with. With several there
/// is no single right place, and the links stay in the message.
pub async fn send(state: &App, digest: &Digest) -> Result<(), AppError> {
    let Some(url) = state.config.notify_url.as_deref() else { return Ok(()) };
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| AppError::Internal(format!("notify client: {e}")))?;
    let request = if state.config.notify_format == "text" {
        let request = client.post(url).header("Title", &digest.title).body(digest.message.clone());
        match digest.reminders.as_slice() {
            [only] => match &only.link {
                Some(link) => request.header("Click", link),
                None => request,
            },
            _ => request,
        }
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
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = $1")
        .bind(LAST_SENT_KEY).fetch_optional(&state.db).await?;
    Ok(row.map(|r| r.0))
}

async fn mark_sent(state: &App, date: &str) -> Result<(), AppError> {
    sqlx::query("INSERT INTO settings (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
        .bind(LAST_SENT_KEY).bind(date).execute(&state.db).await?;
    Ok(())
}

/// One scheduler tick. Returns the digest it sent, if any.
///
/// The day is marked as handled *before* the POST goes out, so an endpoint that is down or
/// misconfigured costs one failed request per day rather than one per minute until midnight.
/// The trade is that a digest lost to a transient failure is not retried; the reminders stay
/// due and appear in tomorrow's.
///
/// That trade is about the POST, and the marker is therefore written only once the digest has
/// actually been built: a failure inside `collect` (a pool timeout, a locked database) means
/// nothing was ever assembled, so marking the day there would drop that day's reminders on
/// the floor without a single request having left the process. A tick that collects nothing
/// still marks the day -- "nothing was due" is a handled day, not a failed one.
pub async fn tick(state: &App, hour_now: u32) -> Result<Option<Digest>, AppError> {
    if state.config.notify_url.is_none() || hour_now < state.config.notify_hour {
        return Ok(None);
    }
    let today = db::today();
    if last_sent(state).await?.as_deref() == Some(today.as_str()) {
        return Ok(None);
    }
    let digest = collect(state).await?;
    mark_sent(state, &today).await?;
    let Some(digest) = digest else { return Ok(None) };
    send(state, &digest).await?;
    Ok(Some(digest))
}
