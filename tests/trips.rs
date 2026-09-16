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

/// Creates a trip with the given date, counters and places -- everything else defaulted, like
/// `trip()` above -- and answers its activity id.
async fn create_trip(app: &common::TestApp, object_id: i64, date: &str, start: i64, end: i64, from: &str, to: &str) -> i64 {
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities"))).json(&json!({
        "date": date, "category": "trip", "title": "", "notes": "",
        "start_counter": start, "counter_value": end, "from_place": from, "to_place": to,
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
}

#[tokio::test]
async fn trip_places_are_distinct_most_recent_first_and_skip_deleted_trips_and_non_trip_entries() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    create_trip(&app, id, "2026-01-01", 0, 100, "Home", "Office").await;
    create_trip(&app, id, "2026-02-01", 100, 200, "Office", "Home").await;
    let gym_trip = create_trip(&app, id, "2026-03-01", 200, 210, "Home", "Gym").await;
    let res = app.client.delete(app.url(&format!("/activities/{gym_trip}"))).send().await.unwrap();
    assert_eq!(res.status(), 204, "{}", res.text().await.unwrap());

    // A non-trip entry, newer than every trip above, to prove it never contributes a place.
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-04-01", "category": "maintenance", "title": "Brakes", "notes": ""
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    let out: Value = app.client.get(app.url(&format!("/objects/{id}/trip-places"))).send().await.unwrap().json().await.unwrap();
    // Most recent trip (2026-02-01) first; the deleted 2026-03-01 trip to "Gym" never appears.
    assert_eq!(out["from"], json!(["Office", "Home"]));
    assert_eq!(out["to"], json!(["Home", "Office"]));
}

/// "Home", "home" and "HOME" are the same place typed three different ways -- see
/// `api::trips::distinct_places`. They must collapse into one suggestion, spelled the way the
/// newest trip wrote it, without disturbing the order of any other (genuinely distinct) place.
#[tokio::test]
async fn trip_places_dedup_case_insensitively_keeping_the_newest_spelling() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    create_trip(&app, id, "2026-01-01", 0, 50, "Work", "HOME").await;
    create_trip(&app, id, "2026-01-02", 50, 100, "Work", "Office").await;
    create_trip(&app, id, "2026-01-03", 100, 150, "Work", "home").await;
    create_trip(&app, id, "2026-01-04", 150, 200, "Work", "Home").await;

    let out: Value = app.client.get(app.url(&format!("/objects/{id}/trip-places"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(
        out["to"], json!(["Home", "Office"]),
        "'Home'/'home'/'HOME' fold into one entry spelled like the newest (2026-01-04) trip; \
         'Office' is unrelated and keeps its own most-recent-first place: {out}"
    );
}

/// The summary the Info tab shows must only ever total live trips -- a soft-deleted trip or an
/// ordinary entry with a big counter jump must not sneak into its counts or distance.
#[tokio::test]
async fn trips_summary_ignores_deleted_trips_and_non_trip_entries() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    create_trip(&app, id, "2026-09-02", 100, 150, "Home", "Office").await; // 50 km, stays live
    let deleted = create_trip(&app, id, "2026-09-03", 150, 500, "Office", "Home").await; // 350 km, deleted below
    let res = app.client.delete(app.url(&format!("/activities/{deleted}"))).send().await.unwrap();
    assert_eq!(res.status(), 204, "{}", res.text().await.unwrap());

    // A big counter jump on a non-trip entry must not be mistaken for trip distance.
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-09-04", "category": "maintenance", "title": "Service", "notes": "", "counter_value": 900
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    let out: Value = app.client
        .get(app.url(&format!("/objects/{id}/trips/summary?today=2026-09-15")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(
        (out["all"]["trips"].as_i64(), out["all"]["distance"].as_i64()), (Some(1), Some(50)),
        "the deleted trip and the maintenance entry must not contribute: {out}"
    );
}

#[tokio::test]
async fn trips_summary_totals_month_year_and_all_time() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    create_trip(&app, id, "2026-09-02", 100, 150, "Home", "Office").await; // this month: 50 km
    create_trip(&app, id, "2026-03-10", 150, 200, "Office", "Home").await; // this year, not this month: 50 km
    create_trip(&app, id, "2025-12-31", 0, 100, "Home", "Office").await; // last year: 100 km

    let out: Value = app.client
        .get(app.url(&format!("/objects/{id}/trips/summary?today=2026-09-15")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!((out["month"]["trips"].as_i64(), out["month"]["distance"].as_i64()), (Some(1), Some(50)));
    assert_eq!((out["year"]["trips"].as_i64(), out["year"]["distance"].as_i64()), (Some(2), Some(100)));
    assert_eq!((out["all"]["trips"].as_i64(), out["all"]["distance"].as_i64()), (Some(3), Some(200)));
}

#[tokio::test]
async fn trips_summary_rejects_a_malformed_today() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    // Single-digit month: chrono's own parsing is lenient enough to accept this, but the
    // downstream `&today[..4]`/`&today[..7]` slices assume the full ten-byte width.
    let short = app.client
        .get(app.url(&format!("/objects/{id}/trips/summary?today=2026-9-15")))
        .send().await.unwrap();
    assert_eq!(short.status(), 400, "{}", short.text().await.unwrap());

    let garbage = app.client
        .get(app.url(&format!("/objects/{id}/trips/summary?today=not-a-date")))
        .send().await.unwrap();
    assert_eq!(garbage.status(), 400, "{}", garbage.text().await.unwrap());
}

#[tokio::test]
async fn trip_places_and_summary_of_another_users_object_are_404() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let bike = create_e_bike(&app).await;
    let id = bike["id"].as_i64().unwrap();

    let res = anna.get(app.url(&format!("/objects/{id}/trip-places"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
    let res = anna.get(app.url(&format!("/objects/{id}/trips/summary"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}
