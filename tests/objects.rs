mod common;
use serde_json::json;

#[tokio::test]
async fn crud_and_stats() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    assert_eq!(car["name"], "Golf");
    assert_eq!(car["counter_unit"], "km");
    assert_eq!(car["stats"]["total_cost_cents"], 0);
    assert_eq!(car["stats"]["activity_count"], 0);
    assert!(car["stats"]["current_counter"].is_null());
    let id = car["id"].as_i64().unwrap();

    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1);

    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf VII", "category": "car", "counter_unit": "km", "description": "grey",
        "purchase_date": "2020-03-01", "purchase_price_cents": 1500000, "archived": true
    })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let car: serde_json::Value = res.json().await.unwrap();
    assert_eq!(car["name"], "Golf VII");
    assert!(car["archived_at"].is_string());

    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 0, "archived hidden by default");
    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects?archived=true")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1);

    assert_eq!(app.client.delete(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 204);
    assert_eq!(app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 404);
}

#[tokio::test]
async fn validation() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for body in [
        json!({ "name": " ", "category": "car" }),
        json!({ "name": "x", "category": "" }),
        json!({ "name": "x", "category": "car", "counter_unit": "furlongs" }),
        json!({ "name": "x", "category": "car", "purchase_date": "01.03.2020" }),
        json!({ "name": "x", "category": "car", "purchase_price_cents": -1 }),
    ] {
        let res = app.client.post(app.url("/objects")).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 400, "{body}");
    }
}

#[tokio::test]
async fn other_users_objects_are_invisible() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let list: Vec<serde_json::Value> = anna.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert!(list.is_empty());
    assert_eq!(anna.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 404);
    assert_eq!(anna.patch(app.url(&format!("/objects/{id}"))).json(&json!({ "name": "pwned", "category": "car" })).send().await.unwrap().status(), 404);
    assert_eq!(anna.delete(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 404);
    assert_eq!(app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn requires_login() {
    let app = common::spawn().await;
    assert_eq!(reqwest::get(app.url("/objects")).await.unwrap().status(), 401);
}

#[tokio::test]
async fn fuel_unit_round_trips_and_is_validated() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "E-bike", "category": "bike", "counter_unit": "km", "fuel_unit": "kwh"
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let bike: serde_json::Value = res.json().await.unwrap();
    assert_eq!(bike["fuel_unit"], "kwh");

    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Car", "category": "car", "fuel_unit": "barrels"
    })).send().await.unwrap();
    assert_eq!(res.status(), 400);
}

async fn due_reminder_count(app: &common::TestApp, object_id: i64) -> i64 {
    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{object_id}")))
        .send().await.unwrap().json().await.unwrap();
    obj["stats"]["due_reminder_count"].as_i64().unwrap()
}

/// The dashboard's "due" chip comes from this same `due_reminder_count`, computed by its own
/// hand-written subquery rather than `domain::reminder::is_due`. A snooze that suppresses
/// `due` on the reminder itself but leaves this count untouched reproduces, on another screen,
/// exactly the "snooze does nothing" bug the snooze rework existed to fix. Covered separately
/// for a date-due and a counter-due reminder, since the subquery has one independent branch
/// for each and gating only one would be a silent half-fix.
#[tokio::test]
async fn due_reminder_count_respects_snooze_for_a_date_due_reminder() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Oil", "due_date": "2020-01-01" }))
        .send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    assert_eq!(due_reminder_count(&app, id).await, 1, "a past due_date counts as due");

    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    assert_eq!(due_reminder_count(&app, id).await, 0, "a snoozed date-due reminder must not count as due");

    // Lapse the snooze by writing an already-past date directly through the pool, the same
    // way tests/reminders.rs does it -- there is no time-travel helper in this harness.
    sqlx::query("UPDATE reminders SET snoozed_until = '2020-01-01' WHERE id = ?")
        .bind(rid)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(due_reminder_count(&app, id).await, 1, "a lapsed snooze must resume counting as due");
}

#[tokio::test]
async fn due_reminder_count_respects_snooze_for_a_counter_due_reminder() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2026-01-01", "category": "maintenance", "title": "service", "counter_value": 60_000 }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);

    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Service", "due_counter": 60_000 }))
        .send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    assert_eq!(due_reminder_count(&app, id).await, 1, "the counter has already been reached");

    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    assert_eq!(due_reminder_count(&app, id).await, 0, "a snoozed counter-due reminder must not count as due");

    sqlx::query("UPDATE reminders SET snoozed_until = '2020-01-01' WHERE id = ?")
        .bind(rid)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(due_reminder_count(&app, id).await, 1, "a lapsed snooze must resume counting as due");
}
