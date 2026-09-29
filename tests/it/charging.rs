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

/// A titled charge, for the tests below that don't care about the title itself -- see
/// `a_fuel_entry_may_have_no_title_like_a_trip` for the blank-title case, which
/// `ActivityInput::validate` now accepts on `fuel` the same way it already does on `trip`.
fn charge(counter: i64, quantity_milli: i64, cost_cents: i64) -> Value {
    json!({
        "date": "2026-09-16", "category": "fuel", "title": "Charge", "notes": "",
        "charged_full": 1, "counter_value": counter, "quantity_milli": quantity_milli, "cost_cents": cost_cents
    })
}

/// The frontend falls back to "Charged"/"Geladen" (or the petrol wording) when a charge's title
/// is blank, mirroring a trip's "Trip" fallback -- see `activityTitle` and `energyLabelKey`. The
/// server itself does no such thing: it just accepts the empty title, same as a trip's.
#[tokio::test]
async fn a_fuel_entry_may_have_no_title_like_a_trip() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();

    let mut body = charge(3420, 8500, 255);
    body["title"] = json!("");
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&body).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let a: Value = res.json().await.unwrap();
    assert_eq!(a["title"], "");
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

/// A full charge, with an optional amount and cost. `counter_value` and `charged_full` are the
/// only fields every charge needs -- `domain::energy`'s own worked example leaves the opening
/// charge of a window without either.
fn full_charge(date: &str, counter: i64, quantity_milli: Option<i64>, cost_cents: Option<i64>) -> Value {
    let mut body = json!({
        "date": date, "category": "fuel", "title": "Charge", "notes": "",
        "charged_full": 1, "counter_value": counter
    });
    let obj = body.as_object_mut().unwrap();
    if let Some(q) = quantity_milli {
        obj.insert("quantity_milli".into(), json!(q));
    }
    if let Some(c) = cost_cents {
        obj.insert("cost_cents".into(), json!(c));
    }
    body
}

fn trip(date: &str, start: i64, end: i64, battery_used_pct: Option<i64>) -> Value {
    json!({
        "date": date, "category": "trip", "title": "", "notes": "",
        "start_counter": start, "counter_value": end, "battery_used_pct": battery_used_pct
    })
}

/// `GET /objects/{id}/energy`'s figures -- see `docs/superpowers/specs/2026-09-16-charging-energy-design.md`
/// and `domain::energy::energy`, which this endpoint only loads rows for.
#[tokio::test]
async fn energy_endpoint_matches_the_domain_worked_example() {
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

    // The same three full charges as `domain::energy`'s worked example: 1000 -> 1400 -> 1800.
    for body in [
        full_charge("2026-09-01", 1000, None, None),
        full_charge("2026-09-05", 1400, Some(8000), Some(240)),
        full_charge("2026-09-10", 1800, Some(8000), Some(260)),
    ] {
        let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    // Two trips after the last full charge, carrying battery_used_pct 30 and 25 over 60 and 50
    // counter units -- the same figures as `domain::energy`'s battery test.
    for body in [
        trip("2026-09-12", 1800, 1860, Some(30)),
        trip("2026-09-14", 1860, 1910, Some(25)),
    ] {
        let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let out = app.get_json(&format!("/objects/{id}/energy")).await;
    assert_eq!(out["unit"], "kwh");
    assert_eq!(out["price_milli"], 30000);
    // Mean window distance: 400.
    assert_eq!(out["distance_per_charge"], 400);
    // Mean of 400*1_000_000/8000 twice: 50_000.
    assert_eq!(out["distance_per_unit_milli"], 50_000);
    // Mean of 240*1000/400 (600) and 260*1000/400 (650): 625.
    assert_eq!(out["cost_per_counter_milli"], 625);
    // used = 30 + 25 = 55, remaining = 45; km_per_pct = (60+50)/(30+25) = 2; range_left = 90.
    assert_eq!(out["battery"]["remaining_pct"], 45);
    assert_eq!(out["battery"]["range_left"], 90);
    assert_eq!(out["battery"]["warn"], false);
}

#[tokio::test]
async fn energy_of_another_users_object_is_404() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();

    let anna = app.create_user_client("anna", "password123").await;
    let res = anna.get(app.url(&format!("/objects/{id}/energy"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn an_object_with_no_charges_has_every_energy_figure_null() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, None).await;
    let id = car["id"].as_i64().unwrap();

    let out = app.get_json(&format!("/objects/{id}/energy")).await;
    assert!(out["unit"].is_null());
    assert!(out["price_milli"].is_null());
    assert!(out["distance_per_charge"].is_null());
    assert!(out["distance_per_unit_milli"].is_null());
    assert!(out["cost_per_counter_milli"].is_null());
    assert!(out["battery"].is_null());
}

#[tokio::test]
async fn energy_endpoint_ignores_deleted_charges_and_trips() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = create_car(&app, Some("kwh")).await;
    let id = car["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&full_charge("2026-09-01", 1000, None, None)).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&full_charge("2026-09-05", 1200, Some(6000), Some(180))).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    // A third full charge that, if it survived, would extend the window and skew every rate --
    // deleted straight after creation.
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&full_charge("2026-09-10", 5000, Some(1), Some(999_999))).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let doomed_charge: Value = res.json().await.unwrap();
    let res = app.client.delete(app.url(&format!("/activities/{}", doomed_charge["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    // The only trip that carries a battery figure -- deleted, so no battery block should remain.
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&trip("2026-09-07", 1200, 1250, Some(10))).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let doomed_trip: Value = res.json().await.unwrap();
    let res = app.client.delete(app.url(&format!("/activities/{}", doomed_trip["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    let out = app.get_json(&format!("/objects/{id}/energy")).await;
    // Only the surviving window (1000 -> 1200, 200 units, 6000 milli-units, 180 cents) counts.
    assert_eq!(out["distance_per_charge"], 200, "the deleted charge must not extend the window");
    // 200*1_000_000/6000 = 33_333 (truncated; 33.333 distance/unit, x1000).
    assert_eq!(out["distance_per_unit_milli"], 33_333);
    // 180*1000/200 = 900 (cents x1000 per counter unit).
    assert_eq!(out["cost_per_counter_milli"], 900);
    assert!(out["battery"].is_null(), "the only trip with a battery figure was deleted");
}
