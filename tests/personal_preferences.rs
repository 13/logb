mod common;
use serde_json::json;

#[tokio::test]
async fn appearance_and_delivery_hour_are_validated_and_private() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert!(app.get_json("/me/appearance").await.is_null());
    let appearance = json!({"locale":"de", "theme":"dark", "dateFormat":"dmy-dot", "firstDayOfWeek":"sunday"});
    let response = app.client.put(app.url("/me/appearance")).json(&appearance).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(app.get_json("/me/appearance").await, appearance);
    let mut invalid = appearance.clone(); invalid["firstDayOfWeek"] = json!("friday");
    assert_eq!(app.client.put(app.url("/me/appearance")).json(&invalid).send().await.unwrap().status(), 400);
    assert_eq!(app.get_json("/me/appearance").await, appearance);
    assert_eq!(app.client.put(app.url("/me/notifications/hour")).json(&json!({"hour":25})).send().await.unwrap().status(), 400);
    assert_eq!(app.client.put(app.url("/me/notifications/hour")).json(&json!({"hour":17})).send().await.unwrap().status(), 200);
    assert_eq!(app.get_json("/me/notifications").await["hour"], 17);
    app.client.post(app.url("/users")).json(&json!({"username":"other", "password":"correct horse"})).send().await.unwrap();
    let other = reqwest::Client::builder().cookie_store(true).build().unwrap();
    assert_eq!(other.post(app.url("/auth/login")).json(&json!({"username":"other", "password":"correct horse"})).send().await.unwrap().status(), 200);
    let theirs: serde_json::Value = other.get(app.url("/me/appearance")).send().await.unwrap().json().await.unwrap();
    assert!(theirs.is_null());
    let notification: serde_json::Value = other.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap();
    assert!(notification["deliveries"].as_array().unwrap().is_empty());
    assert_ne!(notification["hour"], 17);
}
