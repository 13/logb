mod common;
use serde_json::json;

#[tokio::test]
async fn status_reports_setup_required_until_first_user() {
    let app = common::spawn().await;
    let s: serde_json::Value = reqwest::get(app.url("/auth/status")).await.unwrap().json().await.unwrap();
    assert_eq!(s["setup_required"], true);
    app.setup("ben", "correct horse").await;
    let s: serde_json::Value = reqwest::get(app.url("/auth/status")).await.unwrap().json().await.unwrap();
    assert_eq!(s["setup_required"], false);
}

#[tokio::test]
async fn setup_creates_admin_and_logs_in() {
    let app = common::spawn().await;
    let user = app.setup("ben", "correct horse").await;
    assert_eq!(user["username"], "ben");
    assert_eq!(user["is_admin"], true);
    let me: serde_json::Value = app.client.get(app.url("/auth/me")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["username"], "ben");
}

#[tokio::test]
async fn setup_refused_once_a_user_exists() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = common::new_client()
        .post(app.url("/auth/setup"))
        .json(&json!({ "username": "eve", "password": "password123" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 409);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "conflict");
}

#[tokio::test]
async fn setup_validates_credentials() {
    let app = common::spawn().await;
    for (u, p) in [("ab", "password123"), ("bad name", "password123"), ("ben", "short")] {
        let res = app.client.post(app.url("/auth/setup")).json(&json!({ "username": u, "password": p })).send().await.unwrap();
        assert_eq!(res.status(), 400, "{u}/{p}");
    }
}

#[tokio::test]
async fn login_logout_cycle() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let c = common::new_client();
    assert_eq!(app.client.get(app.url("/auth/me")).send().await.unwrap().status(), 200);
    assert_eq!(c.get(app.url("/auth/me")).send().await.unwrap().status(), 401);

    let res = app.login(&c, "ben", "wrong").await;
    assert_eq!(res.status(), 401);
    let res = app.login(&c, "BEN", "correct horse").await; // username is case-insensitive
    assert_eq!(res.status(), 200);
    assert!(res.headers().get("set-cookie").unwrap().to_str().unwrap().contains("memto_session="));
    assert_eq!(c.get(app.url("/auth/me")).send().await.unwrap().status(), 200);

    assert_eq!(c.post(app.url("/auth/logout")).send().await.unwrap().status(), 204);
    assert_eq!(c.get(app.url("/auth/me")).send().await.unwrap().status(), 401);
}

#[tokio::test]
async fn login_is_rate_limited() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let c = common::new_client();
    for _ in 0..10 {
        assert_eq!(app.login(&c, "ben", "wrong").await.status(), 401);
    }
    assert_eq!(app.login(&c, "ben", "correct horse").await.status(), 429);
}
