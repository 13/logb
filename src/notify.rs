//! Daily digest of due reminders, to webhooks and to browsers.
//!
//! LogB has no mail transport. A digest goes to a URL someone already runs -- an ntfy topic, a
//! chat webhook, a home-automation endpoint -- and, for anyone who turned it on, to their browser
//! as a push notification. Every recipient is decided at the same daily tick:
//!
//! - a user with a webhook of their own gets a digest of their own reminders, in their language;
//! - the instance webhook (`LOGB_NOTIFY_URL`) gets everybody else's, as it always has;
//! - every browser a user subscribed gets that user's digest as a push notification.

use crate::api::reminders::due_for_user;
use crate::db;
use crate::domain::reminder::KIND_READING;
use crate::error::AppError;
use crate::push::{self, Delivery, Subscription};
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
    pub object_type: String,
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

/// The digest's words, in the two languages the app speaks. A user's `lang` decides; anything
/// else reads English, the same fallback the interface has.
struct Words {
    one_due: &'static str,
    many_due: &'static str,
    readings: &'static str,
    test_title: &'static str,
    test_body: &'static str,
}

fn words(lang: &str) -> Words {
    match lang {
        "de" => Words {
            one_due: "LogB: 1 Erinnerung fällig",
            many_due: "LogB: {n} Erinnerungen fällig",
            readings: "Zählerstände erfassen:",
            test_title: "LogB: Test-Benachrichtigung",
            test_body: "Benachrichtigungen von LogB kommen hier an.",
        },
        _ => Words {
            one_due: "LogB: 1 reminder due",
            many_due: "LogB: {n} reminders due",
            readings: "Readings to log:",
            test_title: "LogB: test notification",
            test_body: "Notifications from LogB reach you here.",
        },
    }
}

fn link(public_url: Option<&str>, object_id: i64, kind: &str, object_type: &str) -> Option<String> {
    let base = public_url?.trim_end_matches('/');
    Some(format!("{base}{}", path(object_id, kind, object_type)))
}

/// The same place as `link`, inside the app. A push notification opens it against the service
/// worker's own origin, so it needs no public URL at all.
fn path(object_id: i64, kind: &str, object_type: &str) -> String {
    if kind == KIND_READING {
        if object_type == "body" {
            return format!("/objects/{object_id}/activities/new?category=weight");
        }
        format!("/objects/{object_id}/reading")
    } else {
        format!("/objects/{object_id}?tab=reminders")
    }
}

/// One user, as the digest sees them.
#[derive(sqlx::FromRow)]
struct Recipient {
    id: i64,
    username: String,
    lang: String,
    notify_url: Option<String>,
    notify_format: String,
}

async fn recipients(state: &App) -> Result<Vec<Recipient>, AppError> {
    Ok(sqlx::query_as::<_, Recipient>(
        "SELECT id, username, lang, notify_url, notify_format FROM users ORDER BY username",
    )
    .fetch_all(&state.db)
    .await?)
}

async fn items_for(state: &App, r: &Recipient) -> Result<Vec<DueItem>, AppError> {
    let public_url = state.config.public_url.as_deref();
    Ok(due_for_user(state, r.id, 0)
        .await?
        .into_iter()
        .map(|d| DueItem {
            username: r.username.clone(),
            object_id: d.row.object_id,
            link: link(public_url, d.row.object_id, &d.row.kind, &d.row.object_type),
            object_name: d.row.object_name,
            reminder_id: d.row.id,
            title: d.row.title,
            due_date: d.next_due_date,
            due_counter: d.row.due_counter,
            kind: d.row.kind,
            object_type: d.row.object_type,
        })
        .collect())
}

/// Builds the digest for `items` in `lang`, or `None` when there is nothing to say.
pub fn digest(items: Vec<DueItem>, lang: &str) -> Option<Digest> {
    if items.is_empty() {
        return None;
    }
    let w = words(lang);
    let title = if items.len() == 1 {
        w.one_due.to_string()
    } else {
        w.many_due.replace("{n}", &items.len().to_string())
    };

    // Services first; readings below under a heading of their own, because "log the odometer" is
    // a thirty-second job and reads differently next to "brakes are due". A reading's line
    // carries its link, so a phone notification opens straight into the form.
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
        message.push_str(w.readings);
        message.push('\n');
        message.push_str(&readings.join("\n"));
    }
    Some(Digest {
        title,
        message,
        reminders: items,
    })
}

/// A digest with nothing due in it, for "send a test notification".
pub fn test_digest(lang: &str) -> Digest {
    let w = words(lang);
    Digest {
        title: w.test_title.to_string(),
        message: w.test_body.to_string(),
        reminders: Vec::new(),
    }
}

/// The instance webhook's digest: every user without a webhook of their own, in the language of
/// the first administrator -- the person who set `LOGB_NOTIFY_URL` up.
async fn instance_digest(
    state: &App,
    recipients: &[Recipient],
) -> Result<Option<Digest>, AppError> {
    let mut items = Vec::new();
    for r in recipients.iter().filter(|r| r.notify_url.is_none()) {
        items.extend(items_for(state, r).await?);
    }
    let lang: Option<(String,)> =
        sqlx::query_as("SELECT lang FROM users WHERE is_admin = 1 ORDER BY id LIMIT 1")
            .fetch_optional(&state.db)
            .await?;
    Ok(digest(items, lang.map(|l| l.0).as_deref().unwrap_or("en")))
}

/// The instance webhook's digest, or `None` when nothing in it is due.
///
/// One digest rather than one per user: the instance webhook is the operator's, not each
/// user's, so splitting it would send someone else's reminders to the same endpoint anyway. A
/// user who wants their own takes themselves out of it by setting a webhook of their own.
pub async fn collect(state: &App) -> Result<Option<Digest>, AppError> {
    let recipients = recipients(state).await?;
    instance_digest(state, &recipients).await
}

/// POSTs a digest to one webhook. `text` sends the plain message with the summary in a `Title`
/// header, which is what ntfy-style services render; anything else sends JSON.
///
/// A text digest about exactly one reminder also sends a `Click` header naming its link, so
/// tapping the notification opens the app where that reminder is dealt with.
pub async fn post(url: &str, format: &str, digest: &Digest) -> Result<(), AppError> {
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| AppError::Internal(format!("notify client: {e}")))?;
    let request = if format == "text" {
        let request = client
            .post(url)
            .header("Title", &digest.title)
            .body(digest.message.clone());
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
    // `without_url`: a user's webhook is often an unguessable ntfy topic, which is to say a
    // secret, and this error reaches the log.
    let res = request
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("notify post: {}", e.without_url())))?;
    if !res.status().is_success() {
        return Err(AppError::Internal(format!(
            "notify endpoint returned {}",
            res.status()
        )));
    }
    Ok(())
}

/// POSTs the instance digest to `LOGB_NOTIFY_URL`, when it is set.
pub async fn send(state: &App, digest: &Digest) -> Result<(), AppError> {
    let Some(url) = state.config.notify_url.as_deref() else {
        return Ok(());
    };
    post(url, &state.config.notify_format, digest).await
}

/// Pushes one message to every browser `user_id` subscribed, answering how many took it and how
/// many failed. A subscription the push service says is gone is deleted on the way.
pub(crate) async fn push_to(
    state: &App,
    user_id: i64,
    title: &str,
    body: &str,
    open: &str,
) -> Result<(usize, usize), AppError> {
    let subs: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, endpoint, p256dh, auth FROM push_subscriptions WHERE user_id = $1 ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    if subs.is_empty() {
        return Ok((0, 0));
    }
    let kp = push::key_pair(state).await?;
    let contact = push::contact(state);
    let payload = serde_json::json!({ "title": title, "body": body, "url": open }).to_string();
    let (mut sent, mut failed) = (0, 0);
    for (id, endpoint, p256dh, auth) in subs {
        let sub = Subscription {
            endpoint,
            p256dh,
            auth,
        };
        match push::send(&kp, &contact, &sub, payload.as_bytes()).await {
            Delivery::Sent => sent += 1,
            Delivery::Gone => {
                sqlx::query("DELETE FROM push_subscriptions WHERE id = $1")
                    .bind(id)
                    .execute(&state.db)
                    .await?;
            }
            Delivery::Failed(reason) => {
                failed += 1;
                tracing::warn!(user_id, reason, "push notification failed");
            }
        }
    }
    Ok((sent, failed))
}

async fn push_digest(state: &App, user_id: i64, d: &Digest) -> Result<(), AppError> {
    // One reminder opens where it is dealt with; several open the dashboard, which lists them.
    let open = match d.reminders.as_slice() {
        [only] => path(only.object_id, &only.kind, &only.object_type),
        _ => "/".to_string(),
    };
    let (_, failed) = push_to(state, user_id, &d.title, &d.message, &open).await?;
    if failed > 0 {
        return Err(AppError::Internal(format!(
            "{failed} push notification(s) failed"
        )));
    }
    Ok(())
}

async fn last_sent(state: &App) -> Result<Option<String>, AppError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = $1")
        .bind(LAST_SENT_KEY)
        .fetch_optional(&state.db)
        .await?;
    Ok(row.map(|r| r.0))
}

async fn mark_sent(state: &App, date: &str) -> Result<(), AppError> {
    sqlx::query("INSERT INTO settings (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
        .bind(LAST_SENT_KEY).bind(date).execute(&state.db).await?;
    Ok(())
}

/// One scheduler tick. Returns the instance digest it sent, if any.
///
/// Every digest is built before anything is sent, and the day is marked as handled in between:
/// a failure while collecting (a pool timeout, a locked database) means nothing was assembled,
/// so the day is not burnt and the next tick tries again; a failure while *sending* costs one
/// failed request per day per target rather than one per minute until midnight. A digest lost
/// to a failed send is not retried; its reminders stay due and are in tomorrow's.
///
/// Each target is tried even when an earlier one failed -- one person's broken webhook must not
/// cost everybody else their notification -- and the failures are reported together.
pub async fn tick(state: &App, hour_now: u32) -> Result<Option<Digest>, AppError> {
    if hour_now < state.config.notify_hour {
        return Ok(None);
    }
    let today = db::today();
    if last_sent(state).await?.as_deref() == Some(today.as_str()) {
        return Ok(None);
    }

    let recipients = recipients(state).await?;
    let instance = match state.config.notify_url {
        Some(_) => instance_digest(state, &recipients).await?,
        None => None,
    };
    let pushing: Vec<i64> = sqlx::query_scalar("SELECT DISTINCT user_id FROM push_subscriptions")
        .fetch_all(&state.db)
        .await?;
    let mut personal = Vec::new();
    let mut pushes = Vec::new();
    let mut telegrams = Vec::new();
    for r in &recipients {
        let wants_push = pushing.contains(&r.id);
        let has_telegram = crate::telegram::status(state, r.id).await?.is_some();
        if r.notify_url.is_none() && !wants_push && !has_telegram {
            continue;
        }
        let Some(d) = digest(items_for(state, r).await?, &r.lang) else {
            continue;
        };
        if let Some(url) = &r.notify_url {
            personal.push((url.clone(), r.notify_format.clone(), d.clone()));
        }
        if wants_push {
            pushes.push((r.id, d.clone()));
        }
        if has_telegram { telegrams.push((r.id, d)); }
    }
    mark_sent(state, &today).await?;

    let mut failures = Vec::new();
    if let Some(d) = &instance {
        if let Err(e) = send(state, d).await {
            failures.push(e.to_string());
        }
    }
    for (url, format, d) in &personal {
        if let Err(e) = post(url, format, d).await {
            failures.push(e.to_string());
        }
    }
    for (user_id, d) in &pushes {
        if let Err(e) = push_digest(state, *user_id, d).await {
            failures.push(e.to_string());
        }
    }
    for (user_id, d) in &telegrams {
        if let Err(e) = crate::telegram::send_user(state, *user_id, d).await { failures.push(e.to_string()); }
    }
    if !failures.is_empty() {
        return Err(AppError::Internal(failures.join("; ")));
    }
    Ok(instance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: &str) -> DueItem {
        DueItem {
            username: "ben".into(),
            object_id: 1,
            object_name: "Golf".into(),
            reminder_id: 1,
            title: "Oil".into(),
            due_date: None,
            due_counter: None,
            kind: kind.into(),
            object_type: "car".into(),
            link: None,
        }
    }

    #[test]
    fn the_digest_speaks_the_recipients_language() {
        let d = digest(vec![item("service"), item("reading")], "de").unwrap();
        assert_eq!(d.title, "LogB: 2 Erinnerungen fällig");
        assert_eq!(d.message, "Golf: Oil\n\nZählerstände erfassen:\nGolf: Oil");
        assert_eq!(
            digest(vec![item("service")], "de").unwrap().title,
            "LogB: 1 Erinnerung fällig"
        );
    }

    #[test]
    fn an_unknown_language_reads_english() {
        assert_eq!(
            digest(vec![item("service")], "fr").unwrap().title,
            "LogB: 1 reminder due"
        );
        assert!(digest(Vec::new(), "en").is_none());
    }
}
