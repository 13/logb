//! A charge is a `fuel` activity that can be marked `charged_full`, and an object with a
//! `fuel_unit` can carry a price per unit (`energy_price_milli`) -- see
//! `docs/superpowers/specs/2026-09-16-charging-energy-design.md`.

mod common;
use serde_json::{json, Value};

/// Creates an object with the given `fuel_unit` (or none) and a `km` counter -- the fixture
/// every test below uses. `app.create_object` has no `fuel_unit` parameter, hence this.
async fn create_car(app: &common::TestApp, fuel_unit: Option<&str>) -> Value {
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": fuel_unit,
        "description": "", "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json().await.unwrap()
}

#[tokio::test]
async fn energy_price_milli_round_trips_and_clears_on_null() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();

    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "kwh",
        "description": "", "purchase_date": null, "purchase_price_cents": null,
        "energy_price_milli": 30000
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: Value = res.json().await.unwrap();
    assert_eq!(out["energy_price_milli"], 30000);

    let read: Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(read["energy_price_milli"], 30000, "read returns the price");

    let list: Vec<Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list[0]["energy_price_milli"], 30000, "list returns the price");

    // Omitting the field entirely keeps the stored value -- three-state, like the cover and
    // trip fields.
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "kwh",
        "description": "", "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: Value = res.json().await.unwrap();
    assert_eq!(out["energy_price_milli"], 30000, "omitted key keeps the stored price");

    // An explicit null clears it.
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "kwh",
        "description": "", "purchase_date": null, "purchase_price_cents": null,
        "energy_price_milli": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: Value = res.json().await.unwrap();
    assert!(out["energy_price_milli"].is_null(), "explicit null clears it");
}

#[tokio::test]
async fn energy_price_milli_must_be_non_negative() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "kwh",
        "description": "", "purchase_date": null, "purchase_price_cents": null,
        "energy_price_milli": -1
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["message"], "energy_price_milli must be >= 0");
}

#[tokio::test]
async fn energy_price_milli_needs_a_fuel_unit() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // No fuel_unit at all on create.
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km",
        "description": "", "purchase_date": null, "purchase_price_cents": null,
        "energy_price_milli": 30000
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["message"], "energy_price_milli needs a fuel unit");

    // Clearing fuel_unit while a price is set, in the same PATCH body, without also nulling
    // the price -- the price is still there, and now has nothing to price.
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "kwh",
        "description": "", "purchase_date": null, "purchase_price_cents": null,
        "energy_price_milli": 30000
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": null,
        "description": "", "purchase_date": null, "purchase_price_cents": null,
        "energy_price_milli": 30000
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["message"], "energy_price_milli needs a fuel unit");

    // The stored price must not have moved.
    let read: Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(read["energy_price_milli"], 30000);
    assert_eq!(read["fuel_unit"], "kwh");

    // The same rule when the PATCH omits `energy_price_milli` entirely, relying on the stored
    // price (three-state, kept when absent) rather than resending it -- the price is still
    // there even though this body never mentions it.
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": null,
        "description": "", "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["message"], "energy_price_milli needs a fuel unit");
}

#[tokio::test]
async fn clearing_fuel_unit_while_a_price_is_set_is_allowed_when_the_price_is_also_nulled() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "kwh",
        "description": "", "purchase_date": null, "purchase_price_cents": null,
        "energy_price_milli": 30000
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    // The same PATCH clears both fuel_unit and the price -- no conflict.
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": null,
        "description": "", "purchase_date": null, "purchase_price_cents": null,
        "energy_price_milli": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: Value = res.json().await.unwrap();
    assert!(out["fuel_unit"].is_null());
    assert!(out["energy_price_milli"].is_null());
}

/// A `fuel` entry needs a title like every category but `trip` -- `ActivityInput::validate`'s
/// blank-title exception is `trip`-only.
fn charge(counter: i64, quantity_milli: i64, cost_cents: i64) -> Value {
    json!({
        "date": "2026-09-16", "category": "fuel", "title": "Charge", "notes": "",
        "charged_full": 1, "counter_value": counter, "quantity_milli": quantity_milli, "cost_cents": cost_cents
    })
}

#[tokio::test]
async fn charged_full_round_trips_on_a_fuel_entry() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&charge(3420, 8500, 255)).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let a: Value = res.json().await.unwrap();
    assert_eq!(a["charged_full"], 1);

    // A fuel entry without the flag reads charged_full: 0, not null or absent.
    let mut without_flag = charge(3500, 8000, 240);
    without_flag["charged_full"] = json!(0);
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&without_flag).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let b: Value = res.json().await.unwrap();
    assert_eq!(b["charged_full"], 0);

    // A fuel entry that never mentions charged_full at all also reads 0.
    let mut omitted = charge(3600, 8000, 240);
    omitted.as_object_mut().unwrap().remove("charged_full");
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&omitted).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let c: Value = res.json().await.unwrap();
    assert_eq!(c["charged_full"], 0, "omitted on create defaults to 0");

    let read: Value = app.client.get(app.url(&format!("/activities/{}", a["id"]))).send().await.unwrap().json().await.unwrap();
    assert_eq!(read["charged_full"], 1);

    let list: Vec<Value> = app.client.get(app.url(&format!("/objects/{id}/activities"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 3);
}

#[tokio::test]
async fn charged_full_is_refused_on_any_category_but_fuel() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-09-16", "category": "maintenance", "title": "Brakes", "notes": "",
        "charged_full": 1
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["message"], "only a charge can be marked full");
}

#[tokio::test]
async fn patching_a_full_charge_to_another_category_requires_clearing_the_flag() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();

    let created: Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&charge(3420, 8500, 255)).send().await.unwrap().json().await.unwrap();
    let aid = created["id"].as_i64().unwrap();
    assert_eq!(created["charged_full"], 1);

    // Switching category away from fuel with the flag still stored (and not resent) is refused.
    let res = app.client.patch(app.url(&format!("/activities/{aid}"))).json(&json!({
        "date": "2026-09-16", "category": "maintenance", "title": "Brakes", "notes": ""
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["message"], "only a charge can be marked full");

    // Explicitly clearing the flag in the same body is accepted.
    let res = app.client.patch(app.url(&format!("/activities/{aid}"))).json(&json!({
        "date": "2026-09-16", "category": "maintenance", "title": "Brakes", "notes": "",
        "charged_full": 0
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let a: Value = res.json().await.unwrap();
    assert_eq!(a["category"], "maintenance");
    assert_eq!(a["charged_full"], 0);
}

#[tokio::test]
async fn patch_omitting_charged_full_keeps_the_stored_value() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();

    let created: Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&charge(3420, 8500, 255)).send().await.unwrap().json().await.unwrap();
    let aid = created["id"].as_i64().unwrap();

    // A PATCH that omits charged_full, but otherwise still names category fuel, keeps the
    // stored value (1), unlike every other full-replace field on this door.
    let mut body = charge(3420, 9000, 260);
    body.as_object_mut().unwrap().remove("charged_full");
    let res = app.client.patch(app.url(&format!("/activities/{aid}"))).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let a: Value = res.json().await.unwrap();
    assert_eq!(a["charged_full"], 1, "omitted on PATCH keeps the stored value");
    assert_eq!(a["quantity_milli"], 9000, "the resent field did change");
}
