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
                let header = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                sink.0.lock().unwrap().push((header("content-type"), header("title"), body));
                "ok"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
    });
    (format!("http://{addr}/hook"), inbox)
}

/// One object with a reminder whose due date is long past.
async fn seed_overdue(app: &common::TestApp) {
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    for title in ["Oil change", "Inspection"] {
        app.client.post(app.url(&format!("/objects/{id}/reminders")))
            .json(&json!({ "title": title, "due_date": "2020-01-01" }))
            .send().await.unwrap();
    }
}

#[tokio::test]
async fn the_digest_posts_every_due_reminder_as_json() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| { c.notify_url = Some(url); c.notify_hour = 8; }).await;
    seed_overdue(&app).await;

    let digest = memto::notify::tick(&app.state, 9).await.unwrap().expect("a digest was due");
    assert_eq!(digest.title, "memto: 2 reminders due");
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
    let app = common::spawn_with(|c| { c.notify_url = Some(url); c.notify_format = "text".into(); }).await;
    seed_overdue(&app).await;

    memto::notify::tick(&app.state, 9).await.unwrap().expect("a digest was due");
    let got = inbox.received();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1, "memto: 2 reminders due");
    assert_eq!(got[0].2, "Golf: Oil change\nGolf: Inspection");
}

#[tokio::test]
async fn the_digest_goes_out_once_a_day_and_not_before_the_configured_hour() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| { c.notify_url = Some(url); c.notify_hour = 8; }).await;
    seed_overdue(&app).await;

    assert!(memto::notify::tick(&app.state, 7).await.unwrap().is_none(), "too early in the day");
    assert!(inbox.received().is_empty());

    assert!(memto::notify::tick(&app.state, 8).await.unwrap().is_some());
    assert!(memto::notify::tick(&app.state, 9).await.unwrap().is_none(), "already sent today");
    assert!(memto::notify::tick(&app.state, 23).await.unwrap().is_none());
    assert_eq!(inbox.received().len(), 1);
}

#[tokio::test]
async fn nothing_due_means_nothing_posted() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| { c.notify_url = Some(url); }).await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Far off", "due_date": "2099-01-01" }))
        .send().await.unwrap();

    assert!(memto::notify::tick(&app.state, 9).await.unwrap().is_none());
    assert!(inbox.received().is_empty());
}

#[tokio::test]
async fn without_a_url_the_scheduler_does_nothing() {
    let app = common::spawn().await;
    seed_overdue(&app).await;
    assert!(memto::notify::tick(&app.state, 23).await.unwrap().is_none());
    assert!(memto::notify::collect(&app.state).await.unwrap().is_some(), "reminders really are due");
}

/// An endpoint that is down must not turn into a request every minute for the rest of the day.
#[tokio::test]
async fn a_failing_endpoint_is_not_retried_until_tomorrow() {
    // Port 1 on loopback refuses connections.
    let app = common::spawn_with(|c| { c.notify_url = Some("http://127.0.0.1:1/hook".into()); }).await;
    seed_overdue(&app).await;

    assert!(memto::notify::tick(&app.state, 9).await.is_err(), "the failure is reported");
    assert!(memto::notify::tick(&app.state, 9).await.unwrap().is_none(), "but the day is done");
}
