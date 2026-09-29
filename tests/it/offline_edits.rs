//! An edit made offline reaches the server later than it was made. It carries `edited_at`, and
//! each field it changes is kept only if nothing newer changed that field in the meantime.

mod common;
use serde_json::json;
use std::time::Duration;

fn now_millis() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

async fn activity(app: &common::TestApp) -> (i64, serde_json::Value) {
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let res = app.client.post(app.url(&format!("/objects/{}/activities", car["id"])))
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "Wipers" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let a: serde_json::Value = res.json().await.unwrap();
    (a["id"].as_i64().unwrap(), a)
}

#[tokio::test]
async fn a_queued_edit_keeps_what_nobody_touched_since_and_loses_what_somebody_did() {
    let app = common::spawn().await;
    let (id, _) = activity(&app).await;
    let url = app.url(&format!("/activities/{id}"));

    // The phone goes offline and fixes the title and adds a note...
    tokio::time::sleep(Duration::from_millis(30)).await;
    let offline_at = now_millis();
    tokio::time::sleep(Duration::from_millis(30)).await;

    // ...while the desktop, online, renames it.
    let res = app.client.patch(&url)
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "Wiper blades" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);

    // The phone reconnects and sends what it queued.
    let res = app.client.patch(&url)
        .json(&json!({
            "date": "2026-09-01", "category": "repair", "title": "Wipers, front",
            "notes": "done at the garage", "edited_at": offline_at,
        }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert_eq!(out["title"], "Wiper blades", "the desktop's rename came later and stays");
    assert_eq!(out["notes"], "done at the garage", "nobody else touched the notes");
}

#[tokio::test]
async fn a_queued_tags_edit_loses_to_a_newer_one() {
    let app = common::spawn().await;
    let (id, _) = activity(&app).await;
    let url = app.url(&format!("/activities/{id}"));

    // The phone goes offline and retags the entry and adds a note...
    tokio::time::sleep(Duration::from_millis(30)).await;
    let offline_at = now_millis();
    tokio::time::sleep(Duration::from_millis(30)).await;

    // ...while the desktop, online, tags it differently.
    let res = app.client.patch(&url)
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "Wipers", "tags": ["Winter"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);

    let res = app.client.patch(&url)
        .json(&json!({
            "date": "2026-09-01", "category": "repair", "title": "Wipers",
            "notes": "done at the garage", "tags": ["Lease"], "edited_at": offline_at,
        }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert_eq!(out["tags"], json!(["Winter"]), "the desktop's tags came later and stay");
    assert_eq!(out["notes"], "done at the garage", "nobody else touched the notes");
}

#[tokio::test]
async fn a_clock_running_ahead_cannot_make_an_edit_unbeatable() {
    let app = common::spawn().await;
    let (id, _) = activity(&app).await;
    let url = app.url(&format!("/activities/{id}"));

    let res = app.client.patch(&url)
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "From the future", "edited_at": "2999-01-01T00:00:00Z" }))
        .send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["title"], "From the future");

    tokio::time::sleep(Duration::from_millis(30)).await;
    let res = app.client.patch(&url)
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "Corrected" }))
        .send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["title"], "Corrected", "a later ordinary edit still wins");
}

#[tokio::test]
async fn edited_at_has_to_be_a_timestamp() {
    let app = common::spawn().await;
    let (id, _) = activity(&app).await;
    let res = app.client.patch(app.url(&format!("/activities/{id}")))
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "x", "edited_at": "yesterday" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 400);
}
