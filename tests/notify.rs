mod common;
use axum::routing::post;
use axum::Router;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

/// A stand-in webhook: records the content type, the `Title` header and the body of every
/// request it is sent.
#[derive(Clone, Default)]
struct Inbox(Arc<Mutex<Vec<(String, String, String)>>>);

impl Inbox {
    fn received(&self) -> Vec<(String, String, String)> {
        self.0.lock().unwrap().clone()
    }
}

/// Starts the receiver and returns its URL alongside the inbox.
async fn webhook() -> (String, Inbox) {
    let inbox = Inbox::default();
    let sink = inbox.clone();
    let app = Router::new().route(
        "/hook",
        post(move |headers: axum::http::HeaderMap, body: String| {
            let sink = sink.clone();
            async move {
                let header = |k: &str| {
                    headers
                        .get(k)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string()
                };
                sink.0
                    .lock()
                    .unwrap()
                    .push((header("content-type"), header("title"), body));
                "ok"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://{addr}/hook"), inbox)
}

/// One object with a reminder whose due date is long past.
async fn seed_overdue(app: &common::TestApp) {
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    for title in ["Oil change", "Inspection"] {
        app.client
            .post(app.url(&format!("/objects/{id}/reminders")))
            .json(&json!({ "title": title, "due_date": "2020-01-01" }))
            .send()
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn personal_delivery_hour_can_be_earlier_than_the_instance_hour() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| c.notify_hour = 18).await;
    seed_overdue(&app).await;
    app.client.put(app.url("/me/notifications")).json(&json!({"url":url,"format":"text"})).send().await.unwrap();
    app.client.put(app.url("/me/notifications/hour")).json(&json!({"hour":6})).send().await.unwrap();
    logb::notify::tick(&app.state, 5).await.unwrap();
    assert!(inbox.received().is_empty());
    logb::notify::tick(&app.state, 6).await.unwrap();
    assert_eq!(inbox.received().len(), 1);
    logb::notify::tick(&app.state, 18).await.unwrap();
    assert_eq!(inbox.received().len(), 1);
    let status = app.get_json("/me/notifications").await;
    assert!(status["deliveries"][0]["last_success"].is_string());
}

#[tokio::test]
async fn failed_targets_retry_without_resending_successful_targets() {
    let (good_url, inbox) = webhook().await;
    let app = common::spawn_with(|c| { c.notify_url = Some("http://127.0.0.1:1/hook".into()); c.notify_hour = 8; }).await;
    seed_overdue(&app).await;
    // Personal delivery succeeds; the instance still has a second user's overdue reminder.
    app.client.put(app.url("/me/notifications")).json(&json!({"url": good_url, "format": "text"})).send().await.unwrap();
    let create = app.client.post(app.url("/users")).json(&json!({"username": "other", "password": "correct horse"})).send().await.unwrap();
    let other: serde_json::Value = create.json().await.unwrap();
    let object = app.create_object(&app.client, "Other car", Some("km")).await;
    app.client.post(app.url(&format!("/objects/{}/reminders", object["id"]))).json(&json!({"title":"Other due", "due_date":"2020-01-01"})).send().await.unwrap();
    sqlx::query("UPDATE objects SET user_id = $1 WHERE id = $2").bind(other["id"].as_i64().unwrap()).bind(object["id"].as_i64().unwrap()).execute(&app.state.db).await.unwrap();
    assert!(logb::notify::tick(&app.state, 9).await.is_err());
    assert_eq!(inbox.received().len(), 1);
    assert!(logb::notify::tick(&app.state, 9).await.unwrap().is_none());
    for expected in [2, 3] {
        sqlx::query("UPDATE notification_deliveries SET attempted_at = '2000-01-01T00:00:00Z' WHERE target = 'instance'").execute(&app.state.db).await.unwrap();
        assert!(logb::notify::tick(&app.state, 9).await.is_err());
        let (attempts,): (i64,) = sqlx::query_as("SELECT attempts FROM notification_deliveries WHERE target = 'instance'").fetch_one(&app.state.db).await.unwrap();
        assert_eq!(attempts, expected);
        assert_eq!(inbox.received().len(), 1);
    }
    sqlx::query("UPDATE notification_deliveries SET attempted_at = '2000-01-01T00:00:00Z' WHERE target = 'instance'").execute(&app.state.db).await.unwrap();
    assert!(logb::notify::tick(&app.state, 9).await.unwrap().is_none());
}

#[tokio::test]
async fn the_digest_posts_every_due_reminder_as_json() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| {
        c.notify_url = Some(url);
        c.notify_hour = 8;
    })
    .await;
    seed_overdue(&app).await;

    let digest = logb::notify::tick(&app.state, 9)
        .await
        .unwrap()
        .expect("a digest was due");
    assert_eq!(digest.title, "LogB: 2 reminders due");
    assert_eq!(digest.message, "Golf: Oil change\nGolf: Inspection");

    let got = inbox.received();
    assert_eq!(got.len(), 1, "exactly one POST");
    assert!(got[0].0.starts_with("application/json"), "{:?}", got[0]);
    let body: serde_json::Value = serde_json::from_str(&got[0].2).unwrap();
    assert_eq!(body["reminders"].as_array().unwrap().len(), 2);
    assert_eq!(body["reminders"][0]["object_name"], "Golf");
    assert_eq!(body["reminders"][0]["username"], "ben");
}

#[tokio::test]
async fn text_format_posts_the_plain_message_with_a_title_header() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| {
        c.notify_url = Some(url);
        c.notify_format = "text".into();
    })
    .await;
    seed_overdue(&app).await;

    logb::notify::tick(&app.state, 9)
        .await
        .unwrap()
        .expect("a digest was due");
    let got = inbox.received();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1, "LogB: 2 reminders due");
    assert_eq!(got[0].2, "Golf: Oil change\nGolf: Inspection");
}

#[tokio::test]
async fn the_digest_goes_out_once_a_day_and_not_before_the_configured_hour() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| {
        c.notify_url = Some(url);
        c.notify_hour = 8;
    })
    .await;
    seed_overdue(&app).await;

    assert!(
        logb::notify::tick(&app.state, 7).await.unwrap().is_none(),
        "too early in the day"
    );
    assert!(inbox.received().is_empty());

    assert!(logb::notify::tick(&app.state, 8).await.unwrap().is_some());
    assert!(
        logb::notify::tick(&app.state, 9).await.unwrap().is_none(),
        "already sent today"
    );
    assert!(logb::notify::tick(&app.state, 23).await.unwrap().is_none());
    assert_eq!(inbox.received().len(), 1);
}

#[tokio::test]
async fn nothing_due_means_nothing_posted() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| {
        c.notify_url = Some(url);
    })
    .await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    app.client
        .post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Far off", "due_date": "2099-01-01" }))
        .send()
        .await
        .unwrap();

    assert!(logb::notify::tick(&app.state, 9).await.unwrap().is_none());
    assert!(inbox.received().is_empty());
}

#[tokio::test]
async fn without_a_url_the_scheduler_does_nothing() {
    let app = common::spawn().await;
    seed_overdue(&app).await;
    assert!(logb::notify::tick(&app.state, 23).await.unwrap().is_none());
    assert!(
        logb::notify::collect(&app.state).await.unwrap().is_some(),
        "reminders really are due"
    );
}

/// An endpoint that is down must not turn into a request every minute for the rest of the day.
#[tokio::test]
async fn a_failing_endpoint_waits_before_retrying() {
    // Port 1 on loopback refuses connections.
    let app = common::spawn_with(|c| {
        c.notify_url = Some("http://127.0.0.1:1/hook".into());
    })
    .await;
    seed_overdue(&app).await;

    assert!(
        logb::notify::tick(&app.state, 9).await.is_err(),
        "the failure is reported"
    );
    assert!(
        logb::notify::tick(&app.state, 9).await.unwrap().is_none(),
        "but the day is done"
    );
}

/// The day is deliberately marked as handled before the POST, so a dead endpoint costs one
/// request per day rather than one per minute. That reasoning covers the POST only: a failure
/// while COLLECTING the digest means no digest was ever built, let alone sent, and burning the
/// day on it silently drops that day's reminders entirely.
#[tokio::test]
async fn a_failure_while_collecting_does_not_burn_the_day() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| {
        c.notify_url = Some(url);
        c.notify_hour = 8;
    })
    .await;
    seed_overdue(&app).await;

    // Break collection only -- `settings` (where the sent-marker lives) stays intact, which is
    // what makes this distinguishable from a tick that could not record anything at all.
    sqlx::query("ALTER TABLE reminders RENAME TO reminders_hidden")
        .execute(&app.state.db)
        .await
        .unwrap();
    assert!(
        logb::notify::tick(&app.state, 9).await.is_err(),
        "collect must surface its failure"
    );
    assert!(inbox.received().is_empty(), "nothing was sent");

    sqlx::query("ALTER TABLE reminders_hidden RENAME TO reminders")
        .execute(&app.state.db)
        .await
        .unwrap();
    let digest = logb::notify::tick(&app.state, 9)
        .await
        .unwrap()
        .expect("the same day must still be retried once collection works again");
    assert_eq!(digest.reminders.len(), 2);
    assert_eq!(inbox.received().len(), 1);
}
