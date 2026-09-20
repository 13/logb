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
        json!({ "name": "x", "type": "car", "energy_price_milli": -1 }),
        json!({ "name": "x", "type": "car", "counter_unit": "km", "energy_price_milli": 30000 }),
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
    assert_eq!(bike["resource_unit"], "kwh");
    assert_eq!(bike["resource_kind"], serde_json::Value::Null);
    assert_eq!(bike["measurement_mode"], serde_json::Value::Null);

    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Car", "type": "car", "fuel_unit": "barrels"
    })).send().await.unwrap();
    assert_eq!(res.status(), 400);
}

/// `energy_price_milli`'s two 400 messages, exactly as the spec states them --
/// `tests/charging.rs` covers the round trip and the cross-field rule with `fuel_unit` in full.
#[tokio::test]
async fn energy_price_milli_error_messages() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "E-bike", "type": "bike", "counter_unit": "km", "fuel_unit": "kwh", "energy_price_milli": -1
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["message"], "energy_price_milli must be >= 0");

    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Car", "type": "car", "counter_unit": "km", "energy_price_milli": 30000
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["message"], "energy_price_milli needs a fuel unit");
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

    // Reading reminders are counted by a different path (`due_readings`), so the matrix carries
    // one of each side: overdue since 2020 with no reading after the activity above, and one
    // whose start is still years away.
    let _reading_due = add_reminder(&app, id, json!({ "title": "Reading due", "kind": "reading", "every_n": 1, "every_unit": "month", "due_date": "2020-01-01" })).await;
    let _reading_later = add_reminder(&app, id, json!({ "title": "Reading later", "kind": "reading", "every_n": 1, "every_unit": "month", "due_date": "2999-01-01" })).await;

    let reminders: Vec<serde_json::Value> = app.client.get(app.url(&format!("/objects/{id}/reminders")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(reminders.len(), 11, "every seeded row must still be present");

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

/// A brand-new object cannot be anyone's ancestor, so creating one only needs to check that
/// the given parent exists, belongs to the caller, and is not deleted -- no cycle is possible
/// yet.
#[tokio::test]
async fn a_nonexistent_parent_is_invalid_on_create() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let ok = logb::sync::record::parent_is_valid(&mut app.state.db.acquire().await.unwrap(), 1, None, 999_999)
        .await
        .unwrap();
    assert!(!ok, "a parent id that does not exist must be invalid");
}

/// The core property this task exists for: reparenting an ancestor underneath its own
/// descendant must be refused, at every depth, including the trivial one-hop case of an object
/// naming itself.
#[tokio::test]
async fn a_cycle_is_refused_at_every_depth() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    let house_id = house["id"].as_i64().unwrap();
    let garage_id = garage["id"].as_i64().unwrap();
    let light_id = light["id"].as_i64().unwrap();

    let mut conn = app.state.db.acquire().await.unwrap();
    // Build House -> Garage -> Light directly, so this test is not entangled with the REST
    // handler this task's function does not yet feed into.
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(house_id).bind(garage_id)
        .execute(&mut *conn).await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(garage_id).bind(light_id)
        .execute(&mut *conn).await.unwrap();

    // Self-parent.
    assert!(!logb::sync::record::parent_is_valid(&mut conn, 1, Some(house_id), house_id).await.unwrap());
    // One hop: House under its own child.
    assert!(!logb::sync::record::parent_is_valid(&mut conn, 1, Some(house_id), garage_id).await.unwrap());
    // Two hops: House under its grandchild.
    assert!(!logb::sync::record::parent_is_valid(&mut conn, 1, Some(house_id), light_id).await.unwrap());
    // The legitimate direction must still work: Light may be reparented onto House directly.
    assert!(logb::sync::record::parent_is_valid(&mut conn, 1, Some(light_id), house_id).await.unwrap());
}

/// Ownership is not optional: a parent id that exists and has no ancestor relationship to the
/// object is still invalid if it belongs to someone else.
#[tokio::test]
async fn a_parent_belonging_to_another_user_is_invalid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let mine = app.create_object(&app.client, "Mine", None).await;
    let anna = app.create_user_client("anna", "password123").await;
    let theirs_res = anna.post(app.url("/objects"))
        .json(&serde_json::json!({ "name": "Theirs", "type": "car", "counter_unit": null, "description": "", "purchase_date": null, "purchase_price_cents": null }))
        .send().await.unwrap();
    let theirs: serde_json::Value = theirs_res.json().await.unwrap();

    let mut conn = app.state.db.acquire().await.unwrap();
    let ok = logb::sync::record::parent_is_valid(
        &mut conn, 1, Some(mine["id"].as_i64().unwrap()), theirs["id"].as_i64().unwrap(),
    ).await.unwrap();
    assert!(!ok, "a parent owned by another account must be invalid regardless of ancestry");
}

/// Deleting an object deletes everything inside it, at every depth, and each descendant's own
/// activities, attachments and reminders go with it -- not just the descendant itself.
#[tokio::test]
async fn deleting_an_object_tombstones_every_descendant_and_their_own_children() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    let house_id = house["id"].as_i64().unwrap();
    let garage_id = garage["id"].as_i64().unwrap();
    let light_id = light["id"].as_i64().unwrap();
    app.create_activity(&light["id"], "Changed the bulb").await;

    let mut conn = app.state.db.acquire().await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(house_id).bind(garage_id)
        .execute(&mut *conn).await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(garage_id).bind(light_id)
        .execute(&mut *conn).await.unwrap();
    drop(conn);

    let res = app.client.delete(app.url(&format!("/objects/{house_id}"))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    let mut conn = app.state.db.acquire().await.unwrap();
    for id in [garage_id, light_id] {
        let deleted_at: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM objects WHERE id = $1")
            .bind(id).fetch_one(&mut *conn).await.unwrap();
        assert!(deleted_at.is_some(), "object {id} must be tombstoned when its ancestor is deleted");
    }
    let live_activities: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activities WHERE object_id = $1 AND deleted_at IS NULL")
        .bind(light_id).fetch_one(&mut *conn).await.unwrap();
    assert_eq!(live_activities, 0, "the light's own activity must be tombstoned too, not just the light");
}

/// Each cascaded descendant must appear in the change log as its own delete, or a device that
/// only pulls Garage's delete would never learn the light inside it was removed.
#[tokio::test]
async fn a_cascaded_descendant_is_logged_as_its_own_delete() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    let garage_id = garage["id"].as_i64().unwrap();
    let light_id = light["id"].as_i64().unwrap();
    let mut conn = app.state.db.acquire().await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(garage_id).bind(light_id)
        .execute(&mut *conn).await.unwrap();
    drop(conn);

    app.client.delete(app.url(&format!("/objects/{garage_id}"))).send().await.unwrap();

    let light_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(light_id).fetch_one(&app.state.db).await.unwrap();
    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE entity = 'object' AND entity_uuid = $1 AND op = 'delete'")
        .bind(&light_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1, "the light's own delete must reach the change log, or a device never learns it is gone");
}

/// A parent's delete must be logged before any of its descendants', at every level.
///
/// `changes.seq` is assigned by insertion order under the write lock, and a device applies the
/// pull stream in `seq` order. If a child's delete ever carried a LOWER seq than its own
/// parent's, that device would apply the child's tombstone first -- and the cascade's shape
/// (breadth-first over a worklist, since the recursion became a loop) is the only thing that
/// guarantees it does not. A node is dequeued, and so has its own children discovered and
/// logged, strictly after the iteration that enqueued it; this pins that property in behaviour
/// rather than leaving it to be re-derived from the loop.
#[tokio::test]
async fn a_cascaded_parents_delete_is_logged_before_its_childrens() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let mut ids = Vec::new();
    for level in 0..4 {
        let object = app.create_object(&app.client, &format!("Level {level}"), None).await;
        ids.push(object["id"].as_i64().unwrap());
    }
    let mut conn = app.state.db.acquire().await.unwrap();
    for pair in ids.windows(2) {
        sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2")
            .bind(pair[0]).bind(pair[1]).execute(&mut *conn).await.unwrap();
    }
    drop(conn);

    let res = app.client.delete(app.url(&format!("/objects/{}", ids[0]))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    let mut seqs = Vec::new();
    for id in &ids {
        let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
            .bind(id).fetch_one(&app.state.db).await.unwrap();
        let seq: i64 = sqlx::query_scalar(
            "SELECT seq FROM changes WHERE entity = 'object' AND entity_uuid = $1 AND op = 'delete'")
            .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
        seqs.push(seq);
    }
    for depth in 1..seqs.len() {
        assert!(
            seqs[depth] > seqs[depth - 1],
            "the delete at depth {depth} (seq {}) must be logged AFTER its parent's (seq {}), \
             or a device applying the pull stream in order tombstones a child before its parent: {seqs:?}",
            seqs[depth], seqs[depth - 1],
        );
    }
}

/// Deleting a very deep chain must not overflow the stack.
///
/// The cascade used to call itself once per level -- written as `Box::pin(async move { .. })`
/// on the belief that boxing made the recursion safe. It does not: `Box::pin` heap-allocates
/// the future's *state*, not the `poll` call chain, so every level still cost a stack frame.
/// A chain built through the API (once anything writes `parent_id`) would then let any account
/// abort the whole process -- every user's server, not just their own session -- with one
/// DELETE. This is the reviewer's own recipe: a chain far deeper than the old code survived
/// (it passed at 250 and aborted with SIGABRT at 500), against a loop whose stack usage does
/// not grow with depth at all.
#[tokio::test]
async fn a_deep_chain_does_not_overflow_the_stack() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let root = app.create_object(&app.client, "Root", None).await;
    let mut conn = app.state.db.acquire().await.unwrap();
    let mut prev_id = root["id"].as_i64().unwrap();
    for i in 0..2000 {
        let child = app.create_object(&app.client, &format!("Link {i}"), None).await;
        let child_id = child["id"].as_i64().unwrap();
        sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2")
            .bind(prev_id).bind(child_id).execute(&mut *conn).await.unwrap();
        prev_id = child_id;
    }
    drop(conn);

    let res = app.client.delete(app.url(&format!("/objects/{}", root["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204, "a chain 2000 deep must delete cleanly, not crash the process");
}

#[tokio::test]
async fn creating_an_object_with_a_valid_parent_succeeds() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let res = app.client.post(app.url("/objects"))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let garage: serde_json::Value = res.json().await.unwrap();
    assert_eq!(garage["parent_id"], house["id"]);
}

#[tokio::test]
async fn reparenting_onto_a_descendant_is_refused_with_a_400() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();

    let res = app.client.patch(app.url(&format!("/objects/{}", house["id"])))
        .json(&serde_json::json!({ "name": "House", "type": "home", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": garage["id"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 400);
}

/// Roots-only by default is the dashboard's contract, and it must hold for the overwhelmingly
/// common case of an installation with no hierarchy at all.
#[tokio::test]
async fn listing_objects_with_no_parent_id_query_returns_only_roots() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();

    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects?archived=false"))
        .send().await.unwrap().json().await.unwrap();
    let names: Vec<String> = list.iter().map(|o| o["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, vec!["House"], "the default list must exclude Garage, which has a parent");
}

#[tokio::test]
async fn listing_a_specific_parents_children_returns_only_those() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let bike = app.create_object(&app.client, "Bike", None).await; // stays a root
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();

    let list: Vec<serde_json::Value> = app.client
        .get(app.url(&format!("/objects?archived=false&parent_id={}", house["id"])))
        .send().await.unwrap().json().await.unwrap();
    let names: Vec<String> = list.iter().map(|o| o["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, vec!["Garage"]);
    let _ = bike; // present in the account, absent from this query -- the point being tested
}

#[tokio::test]
async fn reading_an_object_reports_its_ancestor_chain() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();
    app.client.patch(app.url(&format!("/objects/{}", light["id"])))
        .json(&serde_json::json!({ "name": "Main light", "type": "other", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": garage["id"] }))
        .send().await.unwrap();

    let read: serde_json::Value = app.client.get(app.url(&format!("/objects/{}", light["id"])))
        .send().await.unwrap().json().await.unwrap();
    let names: Vec<&str> = read["ancestors"].as_array().unwrap().iter()
        .map(|a| a["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["House", "Garage"], "root first, nearest ancestor last, self excluded");
}

/// The ancestor walk has to *stop* on a cycle, rather than answer it.
///
/// No write path can create one -- `parent_is_valid` refuses the write on both doors -- so the
/// cycle here is planted straight into the table, past the validation, exactly as an import or
/// a hand-edited database could. What is asserted is only that the read returns at all: a
/// `WITH RECURSIVE` walk that halts on a cycle may report a partial chain, and what a partial
/// chain says about a state that should be impossible is not worth pinning down. The failure
/// this guards against is not a wrong answer, it is no answer -- a request that never comes
/// back and a connection held forever.
#[tokio::test]
async fn an_ancestor_walk_over_a_data_level_cycle_still_terminates() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let first = app.create_object(&app.client, "First", None).await;
    let second = app.create_object(&app.client, "Second", None).await;
    let (first, second) = (first["id"].as_i64().unwrap(), second["id"].as_i64().unwrap());
    for (child, parent) in [(first, second), (second, first)] {
        sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2")
            .bind(parent)
            .bind(child)
            .execute(&app.state.db)
            .await
            .unwrap();
    }

    let res = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        app.client.get(app.url(&format!("/objects/{first}"))).send(),
    )
    .await
    .expect("reading an object inside a cycle must return, not walk the cycle forever")
    .unwrap();
    assert_eq!(res.status(), 200);
    let read: serde_json::Value = res.json().await.unwrap();
    let chain = read["ancestors"].as_array().expect("an ancestors array, however truncated");
    assert!(chain.len() <= 2, "the walk must not have gone round the cycle: {chain:?}");
}

/// `all=true` must actually reach past the roots.
///
/// `tests/export.rs` already sends `all=true`, but the object it asserts on is a root, so its
/// assertion holds just as well against a `list` that ignored the flag entirely: deleting
/// `$3 OR` from the `WHERE` clause in `objects::list` failed no test in either suite. This one
/// builds three levels and asks for all of them, so that deletion turns it red (it comes back
/// with House alone) while the default list -- asserted here in the same test, against the same
/// tree -- stays green, which is what says the flag is doing the reaching rather than the
/// filter having been dropped altogether.
#[tokio::test]
async fn listing_with_all_returns_every_object_at_every_depth() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage: serde_json::Value = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Garage", "type": "other", "parent_id": house["id"] }))
        .send().await.unwrap().json().await.unwrap();
    let light: serde_json::Value = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Main light", "type": "other", "parent_id": garage["id"] }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(light["parent_id"], garage["id"], "the three-level tree was not built");

    let all: Vec<serde_json::Value> = app.client.get(app.url("/objects?archived=false&all=true"))
        .send().await.unwrap().json().await.unwrap();
    let names: Vec<&str> = all.iter().map(|o| o["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec!["Garage", "House", "Main light"],
        "`all=true` must return every object regardless of nesting, name-ordered",
    );

    // The same tree without the flag: the dashboard's contract, and the half of this test that
    // stays green when `all` is broken.
    let roots: Vec<serde_json::Value> = app.client.get(app.url("/objects?archived=false"))
        .send().await.unwrap().json().await.unwrap();
    let names: Vec<&str> = roots.iter().map(|o| o["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["House"], "without `all` the list is roots only");
}

/// A PATCH that never mentions `parent_id` must not write back a parent it read before it held
/// the write lock.
///
/// The bug in full needs three overlapping requests: `PATCH /objects/Garage {"name": ...}` --
/// no `parent_id` key -- reads `Garage.parent_id = House` and then waits for the lock; a second
/// request makes Garage a root; a third moves House underneath Garage, which `parent_is_valid`
/// passes honestly, because at that moment Garage's ancestors are just `{Garage}`; and then the
/// first request wakes and writes the parent it read back in step one. House and Garage now
/// name each other. Nothing in the app can see the pair (both drop out of the root-only list),
/// the purge holds each back for the other forever without a word, and `--copy-to` cannot order
/// them, so the documented migration to PostgreSQL aborts on the foreign key.
///
/// Three real requests cannot be made to interleave on demand, so the test *is* the second and
/// third: it holds the write lock itself and performs their two writes on that transaction. The
/// PATCH is real, and blocks on the real lock; only the timing is nailed down. Both backends
/// serialise writes the same way, so this measures the handler rather than the database.
///
/// Move `load_owned_object_on(&mut tx, ..)` back out of the transaction -- a
/// `load_owned_object(&state, ..)` before `begin_write`, as it was -- and this fails on SQLite
/// and PostgreSQL alike, with Garage's parent restored to House on top of House's new parent.
#[tokio::test]
async fn a_patch_that_omits_parent_id_cannot_write_back_a_stale_parent() {
    // Two connections are wanted at once -- the lock the test holds, and the one the blocked
    // request eventually gets -- and the harness's default pool is exactly two. A little room
    // above that keeps the test measuring the handler rather than pool exhaustion.
    let app = common::spawn_with(|c| c.db_pool_size = Some(4)).await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage: serde_json::Value = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Garage", "type": "other", "parent_id": house["id"] }))
        .send().await.unwrap().json().await.unwrap();
    let house_id = house["id"].as_i64().unwrap();
    let garage_id = garage["id"].as_i64().unwrap();
    assert_eq!(garage["parent_id"], house["id"], "Garage must start inside House");

    // The lock the PATCH below will have to wait for, held before it is sent.
    let mut lock = logb::db::begin_write(&app.state.db, app.state.backend).await.unwrap();

    let url = app.url(&format!("/objects/{garage_id}"));
    let client = app.client.clone();
    let patch = tokio::spawn(async move {
        // No `parent_id` key at all: exactly what a hand-written client against the
        // bearer-token API sends when it only means to rename something.
        client.patch(url).json(&json!({ "name": "Garage (renamed)", "type": "other" }))
            .send().await.unwrap()
    });
    // Long enough for the request to have made every read it is going to make before the lock,
    // and to be waiting on it. Generous rather than tight: too short only makes the test pass
    // for the wrong reason, and the fix must hold however long the wait is.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // What the other two requests would have committed while the first waited.
    sqlx::query("UPDATE objects SET parent_id = NULL WHERE id = $1")
        .bind(garage_id).execute(&mut *lock).await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2")
        .bind(garage_id).bind(house_id).execute(&mut *lock).await.unwrap();
    lock.commit().await.unwrap();

    assert_eq!(patch.await.unwrap().status(), 200);

    let garage_parent: Option<i64> =
        sqlx::query_scalar("SELECT parent_id FROM objects WHERE id = $1")
            .bind(garage_id).fetch_one(&app.state.db).await.unwrap();
    let house_parent: Option<i64> =
        sqlx::query_scalar("SELECT parent_id FROM objects WHERE id = $1")
            .bind(house_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(
        house_parent,
        Some(garage_id),
        "the reparenting that committed under the lock must have stuck",
    );
    assert_eq!(
        garage_parent, None,
        "the PATCH re-read Garage under the lock, where it is a root, or it wrote back the \
         parent it read before the lock and closed a House <-> Garage cycle",
    );
}

#[tokio::test]
async fn a_create_may_carry_its_own_client_uuid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "car", "client_uuid": "phone-0001-golf" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let created: serde_json::Value = res.json().await.unwrap();
    // The sync bootstrap is the read path that exposes uuids today.
    let boot: serde_json::Value = app.client.get(app.url("/sync/bootstrap")).send().await.unwrap().json().await.unwrap();
    let mine = boot["objects"].as_array().unwrap().iter()
        .find(|o| o["id"] == created["id"]).expect("the object is in the snapshot");
    assert_eq!(mine["client_uuid"], "phone-0001-golf");
}

#[tokio::test]
async fn replaying_a_create_with_the_same_client_uuid_returns_the_same_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let body = json!({ "name": "Golf", "type": "car", "client_uuid": "phone-0001-golf" });
    let first: serde_json::Value = app.client.post(app.url("/objects")).json(&body).send().await.unwrap().json().await.unwrap();
    let res = app.client.post(app.url("/objects")).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 200, "a replay is not a second create");
    let second: serde_json::Value = res.json().await.unwrap();
    assert_eq!(first["id"], second["id"]);
    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1);
}

#[tokio::test]
async fn a_client_uuid_belonging_to_someone_else_or_to_a_tombstone_is_a_conflict() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let body = json!({ "name": "Golf", "type": "car", "client_uuid": "phone-0001-golf" });
    let created: serde_json::Value = app.client.post(app.url("/objects")).json(&body).send().await.unwrap().json().await.unwrap();

    // Another account, same uuid.
    let other = app.create_user_client("anna", "another horse").await;
    let res = other.post(app.url("/objects")).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 409);

    // The owner deletes it; the uuid now names a tombstone and cannot be revived by a replay.
    app.client.delete(app.url(&format!("/objects/{}", created["id"]))).send().await.unwrap();
    let res = app.client.post(app.url("/objects")).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 409);
}

#[tokio::test]
async fn a_malformed_client_uuid_is_a_bad_request() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for bad in ["short", "has space in it", &"x".repeat(65)] {
        let res = app.client.post(app.url("/objects"))
            .json(&json!({ "name": "Golf", "type": "car", "client_uuid": bad }))
            .send().await.unwrap();
        assert_eq!(res.status(), 400, "{bad:?}");
    }
}

#[tokio::test]
async fn every_object_response_carries_its_client_uuid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let created: serde_json::Value = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "car", "client_uuid": "phone-0001-golf" }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(created["client_uuid"], "phone-0001-golf");
    let read: serde_json::Value = app.client.get(app.url(&format!("/objects/{}", created["id"]))).send().await.unwrap().json().await.unwrap();
    assert_eq!(read["client_uuid"], "phone-0001-golf");
    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list[0]["client_uuid"], "phone-0001-golf");
    // A row the server minted has one too -- some 36-character v4.
    let other: serde_json::Value = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Bike", "type": "bike" })).send().await.unwrap().json().await.unwrap();
    assert_eq!(other["client_uuid"].as_str().unwrap().len(), 36);
}

/// A trip only means anything against a `km`/`mi` counter (`ActivityInput::validate`), so an
/// object that already has one may not have its counter moved away from that pair -- `km` and
/// `mi` stay freely interchangeable, since a trip is equally meaningful under either.
#[tokio::test]
async fn a_counter_unit_cannot_leave_km_mi_while_the_object_has_trips() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let id = bike["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-06-01", "category": "trip", "title": "", "notes": "",
        "start_counter": 400, "counter_value": 600
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    for unit in [json!("h"), json!(null)] {
        let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
            "name": "Tern", "type": "e_bike", "counter_unit": unit, "description": "",
            "purchase_date": null, "purchase_price_cents": null
        })).send().await.unwrap();
        assert_eq!(res.status(), 400, "{unit}: {}", res.text().await.unwrap());
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["message"], "this object has trips; its counter must stay km or mi");
    }

    // km <-> mi stays allowed.
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Tern", "type": "e_bike", "counter_unit": "mi", "description": "",
        "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let updated: serde_json::Value = res.json().await.unwrap();
    assert_eq!(updated["counter_unit"], "mi");

    // Deleting the trip lifts the restriction.
    let acts: Vec<serde_json::Value> = app.client.get(app.url(&format!("/objects/{id}/activities")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(app.client.delete(app.url(&format!("/activities/{}", acts[0]["id"]))).send().await.unwrap().status(), 204);
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Tern", "type": "e_bike", "counter_unit": "h", "description": "",
        "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "no live trips left: {}", res.text().await.unwrap());
}
