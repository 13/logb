mod common;
use serde_json::json;

async fn add_activity(app: &common::TestApp, object_id: i64, date: &str, counter: Option<i64>) -> serde_json::Value {
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({ "date": date, "category": "maintenance", "title": "service", "counter_value": counter }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    res.json().await.unwrap()
}

#[tokio::test]
async fn due_by_date_and_counter_and_repeat() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/reminders"));

    let res = app.client.post(&base).json(&json!({ "title": "Oil", "due_date": "2000-01-01", "due_counter": 110_000, "repeat_months": 12, "repeat_counter": 15_000 })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let oil: serde_json::Value = res.json().await.unwrap();
    assert_eq!(oil["due"], true, "date in the past");
    let res = app.client.post(&base).json(&json!({ "title": "Tyres", "due_counter": 105_000 })).send().await.unwrap();
    let tyres: serde_json::Value = res.json().await.unwrap();
    assert_eq!(tyres["due"], false, "no counter reading yet");
    let res = app.client.post(&base).json(&json!({ "title": "TÜV", "due_date": "2999-01-01" })).send().await.unwrap();
    assert_eq!(res.status(), 201);

    add_activity(&app, id, "2026-01-01", Some(106_000)).await;
    let due: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due")).send().await.unwrap().json().await.unwrap();
    let titles: Vec<&str> = due.iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Oil", "Tyres"]);
    assert_eq!(due[0]["object_name"], "Golf");
    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(obj["stats"]["due_reminder_count"], 2);

    let service = add_activity(&app, id, "2026-02-01", Some(107_000)).await;
    let res = app.client.post(app.url(&format!("/reminders/{}/done", oil["id"]))).json(&json!({ "activity_id": service["id"] })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let done: serde_json::Value = res.json().await.unwrap();
    assert!(done["done"]["done_at"].is_string());
    assert_eq!(done["done"]["done_activity_id"], service["id"]);
    assert_eq!(done["next"]["due_date"], "2027-02-01");
    assert_eq!(done["next"]["due_counter"], 122_000);
    assert_eq!(done["next"]["repeat_months"], 12);

    let res = app.client.post(app.url(&format!("/reminders/{}/done", tyres["id"]))).json(&json!({})).send().await.unwrap();
    let done: serde_json::Value = res.json().await.unwrap();
    assert!(done["next"].is_null(), "no repeat");

    let all: Vec<serde_json::Value> = app.client.get(&base).send().await.unwrap().json().await.unwrap();
    assert_eq!(all.len(), 4);
    let due: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due")).send().await.unwrap().json().await.unwrap();
    assert!(due.is_empty());
}

#[tokio::test]
async fn crud_validation_and_isolation() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let home = app.create_object(&app.client, "Home", None).await;
    let hid = home["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{hid}/reminders"));

    for body in [
        json!({ "title": "x" }),
        json!({ "title": " ", "due_date": "2030-01-01" }),
        json!({ "title": "x", "due_date": "2030/01/01" }),
        json!({ "title": "x", "due_counter": 5 }),
        json!({ "title": "x", "due_date": "2030-01-01", "repeat_months": 0 }),
    ] {
        assert_eq!(app.client.post(&base).json(&body).send().await.unwrap().status(), 400, "{body}");
    }

    let r: serde_json::Value = app.client.post(&base).json(&json!({ "title": "Chimney", "due_date": "2030-01-01" })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    let res = app.client.patch(app.url(&format!("/reminders/{rid}"))).json(&json!({ "title": "Chimney sweep", "notes": "call", "due_date": "2030-02-01" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let r: serde_json::Value = res.json().await.unwrap();
    assert_eq!(r["title"], "Chimney sweep");

    assert_eq!(anna.get(&base).send().await.unwrap().status(), 404);
    assert_eq!(anna.get(app.url(&format!("/reminders/{rid}"))).send().await.unwrap().status(), 404);
    assert_eq!(anna.post(app.url(&format!("/reminders/{rid}/done"))).json(&json!({})).send().await.unwrap().status(), 404);
    assert_eq!(anna.delete(app.url(&format!("/reminders/{rid}"))).send().await.unwrap().status(), 404);
    assert_eq!(app.client.delete(app.url(&format!("/reminders/{rid}"))).send().await.unwrap().status(), 204);
}

/// Completing a reminder twice is a conflict, not a second completion.
#[tokio::test]
async fn a_reminder_cannot_be_completed_twice() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Oil", "due_date": "2020-01-01" }))
        .send().await.unwrap().json().await.unwrap();
    let done_url = app.url(&format!("/reminders/{}/done", r["id"]));

    assert_eq!(app.client.post(&done_url).json(&json!({})).send().await.unwrap().status(), 200);
    let res = app.client.post(&done_url).json(&json!({})).send().await.unwrap();
    assert_eq!(res.status(), 409, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "conflict");
}
