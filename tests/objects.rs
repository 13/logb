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
        "name": "Golf VII", "type": "car", "counter_unit": "km", "description": "grey",
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
        json!({ "name": " ", "type": "car" }),
        json!({ "name": "x", "type": "" }),
        json!({ "name": "x", "type": "car", "counter_unit": "furlongs" }),
        json!({ "name": "x", "type": "car", "purchase_date": "01.03.2020" }),
        json!({ "name": "x", "type": "car", "purchase_price_cents": -1 }),
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
    assert_eq!(anna.patch(app.url(&format!("/objects/{id}"))).json(&json!({ "name": "pwned", "type": "car" })).send().await.unwrap().status(), 404);
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
        "name": "E-bike", "type": "bike", "counter_unit": "km", "fuel_unit": "kwh"
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let bike: serde_json::Value = res.json().await.unwrap();
    assert_eq!(bike["fuel_unit"], "kwh");

    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Car", "type": "car", "fuel_unit": "barrels"
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
    sqlx::query("UPDATE reminders SET snoozed_until = '2020-01-01' WHERE id = $1")
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

    sqlx::query("UPDATE reminders SET snoozed_until = '2020-01-01' WHERE id = $1")
        .bind(rid)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(due_reminder_count(&app, id).await, 1, "a lapsed snooze must resume counting as due");
}

/// Dueness is encoded twice: `domain::reminder::is_due` (used to compute each reminder's `due`
/// flag) and the hand-written SQL subquery behind `due_reminder_count` in `derived()`. They were
/// made to agree during development, but nothing pins them against EACH OTHER, so a future edit
/// to either one can silently diverge from the other -- the visible symptom being a "due" badge
/// on an object card that contradicts the reminders list underneath it.
///
/// This seeds one object with a reminder in every state the two encodings must agree on, then
/// checks the object's `due_reminder_count` against a count computed FROM the reminders
/// endpoint's own `due` flags -- not a hardcoded number -- so the test keeps checking that the
/// two copies agree rather than checking either one against a snapshot.
#[tokio::test]
async fn due_reminder_count_agrees_with_each_reminders_due_flag() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    // One activity gives the object a current counter reading of 60_000, which every
    // counter-based row below is checked against.
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2026-01-01", "category": "maintenance", "title": "service", "counter_value": 60_000 }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    async fn add_reminder(app: &common::TestApp, id: i64, body: serde_json::Value) -> i64 {
        let res = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 201, "{} -- {}", body, res.text().await.unwrap());
        let r: serde_json::Value = res.json().await.unwrap();
        r["id"].as_i64().unwrap()
    }

    let _date_only = add_reminder(&app, id, json!({ "title": "Date only", "due_date": "2020-01-01" })).await;
    let _counter_only = add_reminder(&app, id, json!({ "title": "Counter only", "due_counter": 60_000 })).await;
    let _both = add_reminder(&app, id, json!({ "title": "Both", "due_date": "2020-01-01", "due_counter": 60_000 })).await;
    let _neither = add_reminder(&app, id, json!({ "title": "Neither", "due_date": "2999-01-01", "due_counter": 999_999 })).await;

    let snoozed_future = add_reminder(&app, id, json!({ "title": "Snoozed into the future", "due_date": "2020-01-01" })).await;
    let res = app.client.post(app.url(&format!("/reminders/{snoozed_future}/snooze")))
        .json(&json!({ "days": 30 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let snoozed_lapsed = add_reminder(&app, id, json!({ "title": "Lapsed snooze", "due_date": "2020-01-01" })).await;
    // Write an already-past snoozed_until directly through the pool, the same way
    // tests/reminders.rs lapses a snooze -- there is no time-travel helper in this harness.
    sqlx::query("UPDATE reminders SET snoozed_until = '2020-01-01' WHERE id = $1")
        .bind(snoozed_lapsed)
        .execute(&app.state.db)
        .await
        .unwrap();

    let done = add_reminder(&app, id, json!({ "title": "Already done", "due_date": "2010-01-01" })).await;
    let res = app.client.post(app.url(&format!("/reminders/{done}/done"))).json(&json!({})).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    // Boundary rows: every other row above lands its date safely inside 2020 or 2999, so a
    // `<=` in either half of the SQL subquery mutated to `<` would still pass -- these two pin
    // down the "today" edge itself. `due_date <= today` and a lapsed `snoozed_until <= today`
    // must both still count as due when the boundary date IS today, not just when it is
    // safely in the past.
    let today_str = chrono::Utc::now().date_naive().to_string();
    let _due_exactly_today = add_reminder(&app, id, json!({ "title": "Due exactly today", "due_date": today_str })).await;
    let snooze_lapses_exactly_today = add_reminder(&app, id, json!({ "title": "Snooze lapses exactly today", "due_date": "2020-01-01" })).await;
    sqlx::query("UPDATE reminders SET snoozed_until = $1 WHERE id = $2")
        .bind(&today_str)
        .bind(snooze_lapses_exactly_today)
        .execute(&app.state.db)
        .await
        .unwrap();

    let reminders: Vec<serde_json::Value> = app.client.get(app.url(&format!("/objects/{id}/reminders")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(reminders.len(), 9, "every seeded row must still be present");

    let expected_due = reminders.iter().filter(|r| r["due"].as_bool().unwrap()).count() as i64;
    // A matrix where everything happens to land on the same side of "due" would let the two
    // encodings disagree on individual rows while still matching on the total by coincidence.
    assert!(expected_due > 0 && expected_due < reminders.len() as i64, "the matrix must mix due and not-due rows, got {expected_due} due out of {}", reminders.len());

    let stats_due = due_reminder_count(&app, id).await;
    assert_eq!(
        stats_due, expected_due,
        "object stats.due_reminder_count ({stats_due}) must agree with the reminders list's own `due` flags ({expected_due}); \
         reminders: {reminders:#?}"
    );
}

/// PATCH on an object is a full replace for every field but the cover, so a field the client
/// leaves out is written as NULL rather than kept. That is the contract every read-modify-write
/// caller depends on -- `Documents.svelte`'s "set as cover" used to hand-list the fields and
/// omit `fuel_unit`, quietly moving an e-bike's insights from kWh back to litres. Pinning it
/// here so the semantics can't drift silently under the clients that rely on them.
#[tokio::test]
async fn a_patch_that_omits_a_field_clears_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "E-bike", "type": "e_bike", "counter_unit": "km", "fuel_unit": "kwh"
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let o: serde_json::Value = res.json().await.unwrap();
    let id = o["id"].as_i64().unwrap();
    assert_eq!(o["fuel_unit"], "kwh");

    let res = app.client.patch(app.url(&format!("/objects/{id}")))
        .json(&json!({ "name": "E-bike", "type": "e_bike", "counter_unit": "km" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert!(out["fuel_unit"].is_null(), "an omitted field is cleared, not preserved");

    // The whole object round-trips unchanged when the client does send it back whole.
    let res = app.client.patch(app.url(&format!("/objects/{id}")))
        .json(&json!({ "name": "E-bike", "type": "e_bike", "counter_unit": "km", "fuel_unit": "kwh" }))
        .send().await.unwrap();
    let out: serde_json::Value = res.json().await.unwrap();
    assert_eq!(out["fuel_unit"], "kwh");
}

#[tokio::test]
async fn an_object_is_created_with_a_type() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "car" })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let o: serde_json::Value = res.json().await.unwrap();
    assert_eq!(o["type"], "car");
    assert!(o.get("category").is_none(), "the old field must be gone from the response");
}

#[tokio::test]
async fn an_unknown_type_is_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "spaceship" })).send().await.unwrap();
    assert_eq!(res.status(), 400, "the CHECK would catch it, but a 400 says which field is wrong");
}
