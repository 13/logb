//! A trip is an activity with category `trip`: its end is the existing `counter_value`, its
//! start is the new `start_counter`, and it carries four more optional fields -- see
//! `docs/superpowers/specs/2026-09-15-trip-log-design.md`.

mod common;
use serde_json::{json, Value};

/// Creates an object of the built-in `e_bike` type with a `km` counter, the fixture every test
/// below uses -- a trip is legal only on an object with a distance counter.
async fn create_e_bike(app: &common::TestApp) -> Value {
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Tern", "type": "e_bike", "counter_unit": "km",
        "description": "", "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json().await.unwrap()
}

fn trip(start: i64, end: i64) -> Value {
    json!({
        "date": "2026-06-01", "category": "trip", "title": "", "notes": "",
        "start_counter": start, "counter_value": end,
        "from_place": " Home ", "to_place": "Office",
        "duration_minutes": 75, "battery_used_pct": 32
    })
}

#[tokio::test]
async fn a_trip_round_trips_and_moves_the_objects_counter() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&trip(400, 600)).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let a: Value = res.json().await.unwrap();
    assert_eq!(a["start_counter"], 400);
    assert_eq!(a["counter_value"], 600);
    assert_eq!(a["from_place"], "Home", "trimmed");
    assert_eq!(a["to_place"], "Office");
    assert_eq!(a["duration_minutes"], 75);
    assert_eq!(a["battery_used_pct"], 32);
    // An empty title is accepted on a trip and stored as the server sees it -- the UI shows
    // `$t('cat.trip')` ("Trip"/"Fahrt") in its place, but nothing here invents that text.
    assert_eq!(a["title"], "");

    let obj: Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(obj["stats"]["current_counter"], 600, "the trip's end is the object's counter, like any other reading");
}

/// Every other category still requires a title -- the trip exception in `ActivityInput::validate`
/// must not have loosened the general rule.
#[tokio::test]
async fn only_a_trip_accepts_a_blank_title() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-06-01", "category": "maintenance", "title": "", "notes": ""
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "maintenance still requires a title");
}

#[tokio::test]
async fn blank_places_are_stored_as_null() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    let mut body = trip(400, 600);
    body["from_place"] = json!("   ");
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let a: Value = res.json().await.unwrap();
    assert_eq!(a["from_place"], Value::Null);
    assert_eq!(a["to_place"], "Office", "the other place is untouched");
}

/// Every 400 case from the spec's Global Constraints, each with its own body so a failure names
/// which rule broke. Every response must carry a non-empty `message`.
#[tokio::test]
async fn validation_400s_each_carry_a_message() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();
    let no_unit = app.create_object(&app.client, "Toolbox", None).await;
    let no_unit_id = no_unit["id"].as_i64().unwrap();
    let hour_unit = app.client.post(app.url("/objects")).json(&json!({
        "name": "Treadmill", "type": "appliance", "counter_unit": "h",
        "description": "", "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap().json::<Value>().await.unwrap();
    let hour_unit_id = hour_unit["id"].as_i64().unwrap();

    let mut without_start = trip(400, 600);
    without_start["start_counter"] = Value::Null;
    let mut without_counter_value = trip(400, 600);
    without_counter_value["counter_value"] = Value::Null;
    let mut battery_over = trip(400, 600);
    battery_over["battery_used_pct"] = json!(101);
    let mut battery_under = trip(400, 600);
    battery_under["battery_used_pct"] = json!(-1);
    let mut duration_zero = trip(400, 600);
    duration_zero["duration_minutes"] = json!(0);
    let mut duration_over = trip(400, 600);
    duration_over["duration_minutes"] = json!(10081);
    let mut place_too_long = trip(400, 600);
    place_too_long["from_place"] = json!("x".repeat(81));

    for (object_id, body, why) in [
        (id, without_start, "missing start_counter"),
        (id, without_counter_value, "missing counter_value"),
        (id, trip(700, 600), "start_counter above counter_value"),
        (id, trip(-5, 600), "negative start_counter"),
        (id, battery_over, "battery_used_pct over 100"),
        (id, battery_under, "battery_used_pct negative"),
        (id, duration_zero, "duration_minutes 0"),
        (id, duration_over, "duration_minutes over 10080"),
        (id, place_too_long, "from_place over 80 characters"),
        (hour_unit_id, trip(400, 600), "object counts h, not km/mi"),
        (no_unit_id, trip(400, 600), "object has no counter at all"),
    ] {
        let res = app.client.post(app.url(&format!("/objects/{object_id}/activities"))).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 400, "{why}: {}", res.text().await.unwrap());
        let body: Value = res.json().await.unwrap();
        assert!(body["message"].as_str().is_some_and(|m| !m.is_empty()), "{why}: {body}");
    }

    // `start_counter` set on a non-trip category.
    let mut maintenance_with_start = json!({
        "date": "2026-06-01", "category": "maintenance", "title": "Brakes", "notes": ""
    });
    maintenance_with_start["start_counter"] = json!(100);
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&maintenance_with_start).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    let body: Value = res.json().await.unwrap();
    assert!(body["message"].as_str().is_some_and(|m| !m.is_empty()));
}

#[tokio::test]
async fn patching_a_trip_keeps_the_other_trip_fields() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    let created: Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&trip(400, 600)).send().await.unwrap().json().await.unwrap();
    let aid = created["id"].as_i64().unwrap();

    // The test resends the full representation (this API's PATCH contract, like every other
    // scalar field here) but changes only `to_place`.
    let mut changed_to_place = trip(400, 600);
    changed_to_place["to_place"] = json!("Warehouse");
    let res = app.client.patch(app.url(&format!("/activities/{aid}"))).json(&changed_to_place).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let a: Value = res.json().await.unwrap();
    assert_eq!(a["to_place"], "Warehouse");
    assert_eq!(a["from_place"], "Home", "untouched");
    assert_eq!(a["start_counter"], 400, "untouched");
    assert_eq!(a["duration_minutes"], 75, "untouched");
    assert_eq!(a["battery_used_pct"], 32, "untouched");
}

/// PATCH omitting the trip fields keeps them on a PATCH that also omits `category` -- the
/// existing full-replace fields (`date`, `title`, ...) still have to be resent, but the trip
/// fields are three-state (see `ActivityInput::start_counter`'s doc comment).
#[tokio::test]
async fn patch_omitting_trip_fields_keeps_them() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    let created: Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&trip(400, 600)).send().await.unwrap().json().await.unwrap();
    let aid = created["id"].as_i64().unwrap();

    // `counter_value` is not three-state -- like `date`/`title`/`notes` it is an ordinary
    // full-replace field on this door, so the PATCH still resends it; only the five trip
    // fields below are omitted, to prove that omitting THEM keeps them.
    let res = app.client.patch(app.url(&format!("/activities/{aid}"))).json(&json!({
        "date": "2026-06-01", "category": "trip", "title": "", "notes": "a note", "counter_value": 600
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let a: Value = res.json().await.unwrap();
    assert_eq!(a["notes"], "a note");
    assert_eq!(a["start_counter"], 400, "kept: the key was absent, not null");
    assert_eq!(a["from_place"], "Home", "kept");
    assert_eq!(a["to_place"], "Office", "kept");
    assert_eq!(a["duration_minutes"], 75, "kept");
    assert_eq!(a["battery_used_pct"], 32, "kept");
}

/// Moving a trip to a non-trip category with its trip fields still stored is refused, unless
/// the body explicitly nulls them out.
#[tokio::test]
async fn changing_category_away_from_trip_requires_nulling_the_trip_fields() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    let created: Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&trip(400, 600)).send().await.unwrap().json().await.unwrap();
    let aid = created["id"].as_i64().unwrap();

    let res = app.client.patch(app.url(&format!("/activities/{aid}"))).json(&json!({
        "date": "2026-06-01", "category": "maintenance", "title": "Brakes", "notes": ""
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "the stored trip fields are still there, so the category change is refused");

    let res = app.client.patch(app.url(&format!("/activities/{aid}"))).json(&json!({
        "date": "2026-06-01", "category": "maintenance", "title": "Brakes", "notes": "",
        "start_counter": null, "from_place": null, "to_place": null,
        "duration_minutes": null, "battery_used_pct": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let a: Value = res.json().await.unwrap();
    assert_eq!(a["category"], "maintenance");
    for field in ["start_counter", "from_place", "to_place", "duration_minutes", "battery_used_pct"] {
        assert_eq!(a[field], Value::Null, "{field}");
    }
}

#[tokio::test]
async fn list_and_read_return_the_trip_fields() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    let created: Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&trip(400, 600)).send().await.unwrap().json().await.unwrap();
    let aid = created["id"].as_i64().unwrap();

    let list: Vec<Value> = app.client.get(app.url(&format!("/objects/{id}/activities")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["from_place"], "Home");
    assert_eq!(list[0]["to_place"], "Office");
    assert_eq!(list[0]["duration_minutes"], 75);
    assert_eq!(list[0]["battery_used_pct"], 32);

    let one: Value = app.client.get(app.url(&format!("/activities/{aid}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(one["start_counter"], 400);
    assert_eq!(one["from_place"], "Home");
}
