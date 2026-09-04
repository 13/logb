mod common;
use serde_json::json;

#[tokio::test]
async fn admin_manages_users() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/users")).json(&json!({ "username": "anna", "password": "password123" })).send().await.unwrap();
    assert_eq!(res.status(), 201);
    let anna: serde_json::Value = res.json().await.unwrap();
    assert_eq!(anna["is_admin"], false);

    let list: Vec<serde_json::Value> = app.client.get(app.url("/users")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 2);

    let res = app.client.post(app.url("/users")).json(&json!({ "username": "ANNA", "password": "password123" })).send().await.unwrap();
    assert_eq!(res.status(), 409, "duplicate username, case-insensitive");

    let res = app.client.patch(app.url(&format!("/users/{}", anna["id"]))).json(&json!({ "is_admin": true, "lang": "de" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let anna: serde_json::Value = res.json().await.unwrap();
    assert_eq!(anna["is_admin"], true);
    assert_eq!(anna["lang"], "de");

    let res = app.client.delete(app.url(&format!("/users/{}", anna["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204);
    let res = app.client.delete(app.url("/users/1")).send().await.unwrap();
    assert_eq!(res.status(), 400, "cannot delete yourself");
}

#[tokio::test]
async fn non_admin_is_limited_to_self() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    assert_eq!(anna.get(app.url("/users")).send().await.unwrap().status(), 403);
    assert_eq!(anna.post(app.url("/users")).json(&json!({ "username": "x", "password": "password123" })).send().await.unwrap().status(), 403);
    assert_eq!(anna.patch(app.url("/users/1")).json(&json!({ "lang": "de" })).send().await.unwrap().status(), 403);

    let me: serde_json::Value = anna.get(app.url("/auth/me")).send().await.unwrap().json().await.unwrap();
    let res = anna.patch(app.url(&format!("/users/{}", me["id"]))).json(&json!({ "is_admin": true })).send().await.unwrap();
    assert_eq!(res.status(), 403, "cannot promote self");
    let res = anna.patch(app.url(&format!("/users/{}", me["id"]))).json(&json!({ "lang": "de", "password": "newpassword1" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let c = common::new_client();
    assert_eq!(app.login(&c, "anna", "newpassword1").await.status(), 200);
}

#[tokio::test]
async fn rejected_field_does_not_leave_partial_write() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let me: serde_json::Value = app.client.get(app.url("/auth/me")).send().await.unwrap().json().await.unwrap();

    // Password is valid and would apply first; is_admin is a self-demotion and must be
    // rejected. The password write must not survive that rejection.
    let res = app
        .client
        .patch(app.url(&format!("/users/{}", me["id"])))
        .json(&json!({ "password": "newpass1", "is_admin": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);

    let c = common::new_client();
    assert_eq!(app.login(&c, "ben", "correct horse").await.status(), 200, "original password must still work");
    let c2 = common::new_client();
    assert_eq!(app.login(&c2, "ben", "newpass1").await.status(), 401, "rejected update must not have changed the password");
}

#[tokio::test]
async fn settings_currency() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let s: serde_json::Value = anna.get(app.url("/settings")).send().await.unwrap().json().await.unwrap();
    assert_eq!(s["currency"], "EUR");
    assert_eq!(anna.put(app.url("/settings")).json(&json!({ "currency": "CHF" })).send().await.unwrap().status(), 403);
    assert_eq!(app.client.put(app.url("/settings")).json(&json!({ "currency": "chf" })).send().await.unwrap().status(), 400);
    let s: serde_json::Value = app.client.put(app.url("/settings")).json(&json!({ "currency": "CHF" })).send().await.unwrap().json().await.unwrap();
    assert_eq!(s["currency"], "CHF");
}
