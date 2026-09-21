//! Notifications per person: a user's own webhook in their own language, browser push, and the
//! instance timezone that decides when both go out.
//!
//! The timezone tests change the process-wide timezone (`db::set_timezone`), which is why they
//! live in this binary and not beside tests whose due dates depend on "today". Every due date
//! here is years in the past, so which day it is does not change any answer.

mod common;
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use base64ct::{Base64UrlUnpadded, Encoding as _};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use web_push_native::p256::elliptic_curve::sec1::ToEncodedPoint;
use web_push_native::p256::SecretKey;
use web_push_native::Auth;

/// A stand-in receiver: records the headers and raw body of every POST, and answers with
/// whatever status it is told to.
#[derive(Clone)]
struct Receiver {
    got: Arc<Mutex<Vec<(HeaderMap, Bytes)>>>,
    status: Arc<AtomicU16>,
}

impl Receiver {
    fn received(&self) -> Vec<(HeaderMap, Bytes)> {
        self.got.lock().unwrap().clone()
    }
}

async fn receiver() -> (String, Receiver) {
    let r = Receiver { got: Arc::default(), status: Arc::new(AtomicU16::new(201)) };
    let sink = r.clone();
    let app = Router::new().route(
        "/in",
        post(move |headers: HeaderMap, body: Bytes| {
            let sink = sink.clone();
            async move {
                sink.got.lock().unwrap().push((headers, body));
                StatusCode::from_u16(sink.status.load(Ordering::SeqCst)).unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
    });
    (format!("http://{addr}/in"), r)
}

#[derive(Clone, Default)]
struct TelegramStub {
    updates: Arc<Mutex<Vec<serde_json::Value>>>,
    sent: Arc<Mutex<Vec<serde_json::Value>>>,
}

async fn telegram_stub() -> (String, TelegramStub) {
    let stub = TelegramStub::default();
    let updates = stub.updates.clone();
    let sent = stub.sent.clone();
    let app = Router::new()
        .route("/bot123:secret/getMe", post(|| async { axum::Json(json!({"ok":true,"result":{"username":"logb_test_bot"}})) }))
        .route("/bot123:secret/getUpdates", post(move || {
            let updates = updates.clone();
            async move { axum::Json(json!({"ok":true,"result":updates.lock().unwrap().clone()})) }
        }))
        .route("/bot123:secret/sendMessage", post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let sent = sent.clone();
            async move { sent.lock().unwrap().push(body); axum::Json(json!({"ok":true,"result":{}})) }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    (format!("http://{addr}"), stub)
}

async fn overdue(app: &common::TestApp, client: &reqwest::Client, object: &str, title: &str) {
    let o = app.create_object(client, object, None).await;
    let res = client.post(app.url(&format!("/objects/{}/reminders", o["id"])))
        .json(&json!({ "title": title, "due_date": "2020-01-01" })).send().await.unwrap();
    assert_eq!(res.status(), 201);
}

async fn me(app: &common::TestApp, client: &reqwest::Client) -> serde_json::Value {
    client.get(app.url("/auth/me")).send().await.unwrap().json().await.unwrap()
}

#[tokio::test]
async fn a_user_with_their_own_webhook_gets_their_own_digest_in_their_language() {
    let (instance_url, instance) = receiver().await;
    let (anna_url, anna_inbox) = receiver().await;
    let app = common::spawn_with(|c| { c.notify_url = Some(instance_url); c.notify_format = "text".into(); }).await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    overdue(&app, &app.client, "Golf", "Oil change").await;
    overdue(&app, &anna, "Fahrrad", "Kette ölen").await;

    let anna_id = me(&app, &anna).await["id"].as_i64().unwrap();
    let res = anna.patch(app.url(&format!("/users/{anna_id}"))).json(&json!({ "lang": "de" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let res = anna.put(app.url("/me/notifications")).json(&json!({ "url": anna_url, "format": "text" })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let digest = logb::notify::tick(&app.state, 9).await.unwrap().expect("the instance digest");
    assert_eq!(digest.message, "Golf: Oil change", "anna has left the instance digest");

    let got = instance.received();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1, "Golf: Oil change");

    let got = anna_inbox.received();
    assert_eq!(got.len(), 1, "anna's own webhook");
    assert_eq!(got[0].0["title"], "LogB: 1 Erinnerung fällig");
    assert_eq!(got[0].1, "Fahrrad: Kette ölen");
}

#[tokio::test]
async fn a_webhook_must_be_an_http_address_and_blank_means_none() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for bad in [json!({ "url": "ftp://example.com/x" }), json!({ "url": "not a url" }), json!({ "url": "https://x.example/", "format": "xml" })] {
        let res = app.client.put(app.url("/me/notifications")).json(&bad).send().await.unwrap();
        assert_eq!(res.status(), 400, "{bad}");
    }
    let res = app.client.put(app.url("/me/notifications")).json(&json!({ "url": "  ", "format": "json" })).send().await.unwrap();
    let out: serde_json::Value = res.json().await.unwrap();
    assert!(out["url"].is_null());
    assert_eq!(out["format"], "json");
    assert_eq!(out["instance_webhook"], false);
    assert!(out["vapid_public_key"].as_str().unwrap().len() > 80, "an uncompressed P-256 point, base64url");
}

#[tokio::test]
async fn telegram_link_requires_instance_configuration() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let response = app.client.post(app.url("/me/notifications/telegram/link")).send().await.unwrap();
    assert_eq!(response.status(), 503);
}

#[tokio::test]
async fn a_bot_token_belongs_to_only_one_user() {
    let (api_url, _telegram) = telegram_stub().await;
    let app = common::spawn_with(|c| c.telegram_api_url = api_url).await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    save_telegram(&app, &app.client).await;
    let response = anna.put(app.url("/me/notifications/telegram"))
        .json(&json!({"token":"123:secret"})).send().await.unwrap();
    assert_eq!(response.status(), 409);
}

async fn save_telegram(app: &common::TestApp, client: &reqwest::Client) -> serde_json::Value {
    let response = client.put(app.url("/me/notifications/telegram"))
        .json(&json!({"token":"123:secret"})).send().await.unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    client.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap()
}

#[tokio::test]
async fn telegram_links_a_private_chat_and_sends_to_its_chat_id() {
    let (api_url, telegram) = telegram_stub().await;
    let app = common::spawn_with(|c| c.telegram_api_url = api_url).await;
    app.setup("ben", "correct horse").await;
    let configured = save_telegram(&app, &app.client).await;
    assert_eq!(configured["telegram_bot_username"], "logb_test_bot");
    assert!(configured.get("token").is_none(), "the API never returns the secret");
    let stored: String = sqlx::query_scalar("SELECT token_cipher FROM telegram_credentials").fetch_one(&app.state.db).await.unwrap();
    assert!(!stored.contains("secret"), "the database only contains ciphertext");
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(app.state.config.data_dir.join("telegram.key")).unwrap().permissions().mode() & 0o777, 0o600);
    }

    let link: serde_json::Value = app.client.post(app.url("/me/notifications/telegram/link"))
        .send().await.unwrap().json().await.unwrap();
    assert!(link["qr_svg"].as_str().unwrap().contains("<svg"));
    let deep_link = link["url"].as_str().unwrap();
    let code = deep_link.split("start=").nth(1).unwrap();
    telegram.updates.lock().unwrap().push(json!({
        "update_id": 41,
        "message": {"text": format!("/start {code}"), "chat":{"id":4242,"type":"private"}, "from":{"first_name":"Ada"}}
    }));
    logb::telegram::poll_once(&app.state).await.unwrap();

    let settings: serde_json::Value = app.client.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap();
    assert_eq!(settings["telegram_connected"], true);
    assert_eq!(settings["telegram_display_name"], "Ada");
    let response: serde_json::Value = app.client.post(app.url("/me/notifications/test")).send().await.unwrap().json().await.unwrap();
    assert_eq!(response["telegram"], "sent");
    let sent = telegram.sent.lock().unwrap();
    assert_eq!(sent.last().unwrap()["chat_id"], "4242", "delivery uses the chat id, not the display name");
}

#[tokio::test]
async fn telegram_rejects_group_chats_without_consuming_the_link() {
    let (api_url, telegram) = telegram_stub().await;
    let app = common::spawn_with(|c| c.telegram_api_url = api_url).await;
    app.setup("ben", "correct horse").await;
    save_telegram(&app, &app.client).await;
    let link: serde_json::Value = app.client.post(app.url("/me/notifications/telegram/link"))
        .send().await.unwrap().json().await.unwrap();
    let code = link["url"].as_str().unwrap().split("start=").nth(1).unwrap().to_string();
    telegram.updates.lock().unwrap().push(json!({
        "update_id": 1, "message":{"text":format!("/start {code}"),"chat":{"id":-99,"type":"group"},"from":{"first_name":"Group"}}
    }));
    logb::telegram::poll_once(&app.state).await.unwrap();
    let settings: serde_json::Value = app.client.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap();
    assert_eq!(settings["telegram_connected"], false);

    telegram.updates.lock().unwrap().push(json!({
        "update_id": 2, "message":{"text":format!("/start {code}"),"chat":{"id":55,"type":"private"},"from":{"first_name":"Ben"}}
    }));
    logb::telegram::poll_once(&app.state).await.unwrap();
    let settings: serde_json::Value = app.client.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap();
    assert_eq!(settings["telegram_connected"], true, "the group attempt did not consume the code");
}

#[tokio::test]
async fn telegram_chat_cannot_be_stolen_by_another_account() {
    let (api_url, telegram) = telegram_stub().await;
    let app = common::spawn_with(|c| {
        c.telegram_bot_token = Some("123:secret".into());
        c.telegram_api_url = api_url;
    }).await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let ben_link: serde_json::Value = app.client.post(app.url("/me/notifications/telegram/link"))
        .send().await.unwrap().json().await.unwrap();
    let ben_code = ben_link["url"].as_str().unwrap().split("start=").nth(1).unwrap();
    *telegram.updates.lock().unwrap() = vec![json!({
        "update_id":1,"message":{"text":format!("/start {ben_code}"),"chat":{"id":77,"type":"private"},"from":{"first_name":"Ben"}}
    })];
    logb::telegram::poll_once(&app.state).await.unwrap();

    let anna_link: serde_json::Value = anna.post(app.url("/me/notifications/telegram/link"))
        .send().await.unwrap().json().await.unwrap();
    let anna_code = anna_link["url"].as_str().unwrap().split("start=").nth(1).unwrap();
    *telegram.updates.lock().unwrap() = vec![json!({
        "update_id":2,"message":{"text":format!("/start {anna_code}"),"chat":{"id":77,"type":"private"},"from":{"first_name":"Anna"}}
    })];
    logb::telegram::poll_once(&app.state).await.unwrap();

    let ben_settings: serde_json::Value = app.client.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap();
    let anna_settings: serde_json::Value = anna.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap();
    assert_eq!(ben_settings["telegram_connected"], true);
    assert_eq!(anna_settings["telegram_connected"], false);
}

/// A browser's side of a subscription, with keys this test holds so it can read what arrives.
struct Browser {
    secret: SecretKey,
    auth: [u8; 16],
}

impl Browser {
    fn new() -> Self {
        Browser { secret: SecretKey::from_slice(&[7u8; 32]).unwrap(), auth: [3u8; 16] }
    }

    fn subscription(&self, endpoint: &str) -> serde_json::Value {
        let point = self.secret.public_key().to_encoded_point(false);
        json!({
            "endpoint": endpoint,
            "keys": {
                "p256dh": Base64UrlUnpadded::encode_string(point.as_bytes()),
                "auth": Base64UrlUnpadded::encode_string(&self.auth),
            },
        })
    }

    fn read(&self, body: &[u8]) -> serde_json::Value {
        let plain = web_push_native::decrypt(body.to_vec(), &self.secret, &Auth::from(self.auth)).unwrap();
        serde_json::from_slice(&plain).unwrap()
    }
}

#[tokio::test]
async fn a_subscribed_browser_gets_the_digest_encrypted_to_it() {
    let (push_url, push_service) = receiver().await;
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    overdue(&app, &app.client, "Golf", "Oil change").await;

    let browser = Browser::new();
    let res = app.client.post(app.url("/push/subscriptions")).json(&browser.subscription(&push_url)).send().await.unwrap();
    assert_eq!(res.status(), 204, "{}", res.text().await.unwrap());
    let settings: serde_json::Value = app.client.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap();
    assert_eq!(settings["push_devices"], 1);

    // No instance webhook at all: push alone is reason enough for the tick to run.
    assert!(logb::notify::tick(&app.state, 9).await.unwrap().is_none());
    let got = push_service.received();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0["content-encoding"], "aes128gcm");
    assert!(got[0].0["authorization"].to_str().unwrap().starts_with("vapid t="));
    let message = browser.read(&got[0].1);
    assert_eq!(message["title"], "LogB: 1 reminder due");
    assert_eq!(message["body"], "Golf: Oil change");
    assert!(message["url"].as_str().unwrap().ends_with("?tab=reminders"), "one reminder opens where it is dealt with");

    // "Send a test notification" reaches the same browser, now.
    let res = app.client.post(app.url("/me/notifications/test")).send().await.unwrap();
    let out: serde_json::Value = res.json().await.unwrap();
    assert_eq!(out["push_sent"], 1);
    assert!(out["webhook"].is_null());
    assert_eq!(browser.read(&push_service.received()[1].1)["title"], "LogB: test notification");
}

#[tokio::test]
async fn a_subscription_the_push_service_has_forgotten_is_removed() {
    let (push_url, push_service) = receiver().await;
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.client.post(app.url("/push/subscriptions")).json(&Browser::new().subscription(&push_url)).send().await.unwrap();

    push_service.status.store(410, Ordering::SeqCst);
    let out: serde_json::Value = app.client.post(app.url("/me/notifications/test")).send().await.unwrap().json().await.unwrap();
    assert_eq!((out["push_sent"].as_i64(), out["push_failed"].as_i64()), (Some(0), Some(0)));
    let settings: serde_json::Value = app.client.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap();
    assert_eq!(settings["push_devices"], 0);
}

#[tokio::test]
async fn a_push_endpoint_a_browser_would_never_hand_out_is_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/push/subscriptions"))
        .json(&Browser::new().subscription("http://192.168.1.1/admin")).send().await.unwrap();
    assert_eq!(res.status(), 400);
    // Unsubscribing something never subscribed is the state asked for.
    let res = app.client.delete(app.url("/push/subscriptions")).json(&json!({ "endpoint": "https://nope.example/" })).send().await.unwrap();
    assert_eq!(res.status(), 204);
}

#[tokio::test]
async fn first_run_setup_takes_the_browsers_timezone_and_an_admin_can_change_it() {
    let app = common::spawn().await;
    let res = app.client.post(app.url("/auth/setup"))
        .json(&json!({ "username": "ben", "password": "correct horse", "timezone": "Europe/Berlin" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let settings: serde_json::Value = app.client.get(app.url("/settings")).send().await.unwrap().json().await.unwrap();
    assert_eq!(settings["timezone"], "Europe/Berlin");
    assert_eq!(settings["timezone_locked"], false);

    let res = app.client.put(app.url("/settings")).json(&json!({ "currency": "EUR", "timezone": "Mars/Olympus" })).send().await.unwrap();
    assert_eq!(res.status(), 400);
    let res = app.client.put(app.url("/settings")).json(&json!({ "currency": "CHF", "timezone": "Europe/Zurich" })).send().await.unwrap();
    let settings: serde_json::Value = res.json().await.unwrap();
    assert_eq!((settings["currency"].as_str(), settings["timezone"].as_str()), (Some("CHF"), Some("Europe/Zurich")));
    let stored: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'timezone'").fetch_one(&app.state.db).await.unwrap();
    assert_eq!(stored, "Europe/Zurich", "kept across a restart");
}

#[tokio::test]
async fn logb_timezone_wins_and_settings_says_so() {
    let app = common::spawn_with(|c| c.timezone = Some(chrono_tz::Tz::Asia__Tokyo)).await;
    let res = app.client.post(app.url("/auth/setup"))
        .json(&json!({ "username": "ben", "password": "correct horse", "timezone": "Europe/Berlin" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let settings: serde_json::Value = app.client.get(app.url("/settings")).send().await.unwrap().json().await.unwrap();
    assert_eq!(settings["timezone_locked"], true);
    let res = app.client.put(app.url("/settings")).json(&json!({ "currency": "EUR", "timezone": "Europe/Berlin" })).send().await.unwrap();
    assert_eq!(res.status(), 400, "an environment variable is not overridden from the app");
}
