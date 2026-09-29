use super::common;
use super::helpers::*;
use serde_json::json;


/// Creates a trip, exactly as `POST .../activities` would, and returns its JSON.
async fn create_trip(
    app: &common::TestApp,
    object_id: i64,
    extra: serde_json::Value,
) -> serde_json::Value {
    let mut body = json!({
        "date": "2026-06-01", "category": "trip", "title": "", "notes": "",
        "start_counter": 400, "counter_value": 600
    });
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json().await.unwrap()
}

/// A `create` op for an activity only announces a row REST already made (`apply_op`'s doc
/// comment on `OpKind::Create`), so this REST-creates the trip with a client-minted uuid first,
/// then pushes the announcement -- and pulls it back through bootstrap to check every field.
#[tokio::test]
async fn a_pushed_trip_create_round_trips_every_field() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();

    create_trip(&app, object_id, json!({
        "from_place": "Home", "to_place": "Office", "duration_minutes": 75, "battery_used_pct": 32,
        "client_uuid": "trip-uuid-0001"
    })).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-trip-create", "entity": "activity", "entity_uuid": "trip-uuid-0001",
            "op": "create", "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let boot: serde_json::Value = app
        .client
        .get(app.url("/sync/bootstrap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let a = boot["activities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["client_uuid"] == "trip-uuid-0001")
        .unwrap();
    assert_eq!(a["start_counter"], 400);
    assert_eq!(a["counter_value"], 600);
    assert_eq!(a["from_place"], "Home");
    assert_eq!(a["to_place"], "Office");
    assert_eq!(a["duration_minutes"], 75);
    assert_eq!(a["battery_used_pct"], 32);
}

#[tokio::test]
async fn pushing_start_counter_above_the_stored_counter_value_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    let created = create_trip(&app, object_id, json!({})).await;
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-start-over", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "start_counter", "value": 700,
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "start_counter must be between 0 and counter_value"
    );

    let stored: i64 =
        sqlx::query_scalar("SELECT start_counter FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, 400, "the rejected write must not land");
}

/// The same cross-field rule from the other side: `counter_value` is not trip-only, so it is
/// checked against the row's stored `start_counter` only when the row is a trip.
#[tokio::test]
async fn pushing_counter_value_below_the_stored_start_counter_on_a_trip_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    let created = create_trip(&app, object_id, json!({})).await;
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-end-under", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "counter_value", "value": 300,
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "start_counter must be between 0 and counter_value"
    );
}

#[tokio::test]
async fn pushing_battery_used_pct_is_accepted_and_stamps_the_field_clock() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    let created = create_trip(&app, object_id, json!({})).await;
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-battery", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "battery_used_pct", "value": 50,
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let stored: Option<i64> =
        sqlx::query_scalar("SELECT battery_used_pct FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, Some(50));

    let clocked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM field_clock WHERE entity = 'activity' AND entity_uuid = $1 AND field = 'battery_used_pct'")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(
        clocked, 1,
        "the field clock must be stamped so a later, older write loses to this one"
    );
}

#[tokio::test]
async fn pushing_a_trip_field_on_a_non_trip_row_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({
            "date": "2026-06-01", "category": "maintenance", "title": "Brakes", "notes": ""
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let created: serde_json::Value = res.json().await.unwrap();
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-from-place-non-trip", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "from_place", "value": "Home",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "only a trip has start_counter, places, duration or battery"
    );

    let stored: Option<String> =
        sqlx::query_scalar("SELECT from_place FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(stored.is_none(), "the rejected write must not land");
}

/// `category` itself is settable over sync -- the checks above cover the five trip fields and
/// `counter_value` being set on an established trip; these cover `category` moving a row to or
/// from `trip`, and the two ways a trip may not lose its start once it has one.
#[tokio::test]
async fn pushing_category_to_trip_without_a_stored_start_counter_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    // An ordinary reading: a counter_value, no start_counter -- exactly what every non-trip row
    // looks like, since nothing but a trip may ever have one.
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities"))).json(&json!({
        "date": "2026-06-01", "category": "reading", "title": "Odometer", "notes": "", "counter_value": 500
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let created: serde_json::Value = res.json().await.unwrap();
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-to-trip-no-start", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "category", "value": "trip",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "a trip needs start_counter and counter_value"
    );

    let category: String =
        sqlx::query_scalar("SELECT category FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(category, "reading", "the rejected write must not land");
}

#[tokio::test]
async fn pushing_category_to_trip_on_an_hour_object_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let treadmill = app
        .client
        .post(app.url("/objects"))
        .json(&json!({
            "name": "Treadmill", "type": "appliance", "counter_unit": "h",
            "description": "", "purchase_date": null, "purchase_price_cents": null
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let object_id = treadmill["id"].as_i64().unwrap();
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({
            "date": "2026-06-01", "category": "maintenance", "title": "Belt", "notes": ""
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let created: serde_json::Value = res.json().await.unwrap();
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-to-trip-hour-unit", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "category", "value": "trip",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "a trip needs an object that counts km or mi"
    );
}

#[tokio::test]
async fn pushing_category_away_from_trip_with_fields_still_stored_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    let created = create_trip(&app, object_id, json!({ "from_place": "Home" })).await;
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-away-from-trip-still-stored", "entity": "activity", "entity_uuid": uuid,
        "op": "set", "field": "category", "value": "maintenance",
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "only a trip has start_counter, places, duration or battery"
    );

    let category: String =
        sqlx::query_scalar("SELECT category FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(category, "trip", "the rejected write must not land");
}

#[tokio::test]
async fn pushing_counter_value_to_null_on_a_trip_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    let created = create_trip(&app, object_id, json!({})).await;
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-null-counter-value-on-trip", "entity": "activity", "entity_uuid": uuid,
        "op": "set", "field": "counter_value", "value": null,
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "a trip needs start_counter and counter_value"
    );

    let stored: Option<i64> =
        sqlx::query_scalar("SELECT counter_value FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, Some(600), "the rejected write must not land");
}

#[tokio::test]
async fn pushing_start_counter_to_null_on_a_trip_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    let created = create_trip(&app, object_id, json!({})).await;
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-null-start-counter-on-trip", "entity": "activity", "entity_uuid": uuid,
        "op": "set", "field": "start_counter", "value": null,
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "a trip needs start_counter and counter_value"
    );

    let stored: Option<i64> =
        sqlx::query_scalar("SELECT start_counter FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, Some(400), "the rejected write must not land");
}

/// The positive half of the two rejections above: once a trip's five fields are all cleared --
/// something only a REST PATCH can do in the one atomic step that also changes `category` (see
/// the two tests above; a synced `set` never can, one field at a time) -- a `category` op away
/// from `trip` is legal. The clearing itself is done with a raw UPDATE, standing in for that
/// REST PATCH having already landed, so this test is only about the `category` op that follows.
#[tokio::test]
async fn pushing_category_away_from_trip_once_fields_are_cleared_is_accepted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    let created = create_trip(&app, object_id, json!({ "from_place": "Home" })).await;
    let id = created["id"].as_i64().unwrap();
    let uuid = client_uuid(&app.state.db, "activities", id).await;

    sqlx::query(
        "UPDATE activities SET start_counter = NULL, from_place = NULL, to_place = NULL, \
         duration_minutes = NULL, battery_used_pct = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-away-from-trip-cleared", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "category", "value": "maintenance",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let category: String =
        sqlx::query_scalar("SELECT category FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(category, "maintenance");
}

/// The same rule as `a_counter_unit_cannot_leave_km_mi_while_the_object_has_trips` in
/// `tests/objects.rs`, checked at the other door: `km`/`mi` stay interchangeable, but leaving
/// the pair is rejected while a live trip depends on it.
#[tokio::test]
async fn pushing_counter_unit_away_from_km_mi_with_a_live_trip_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    create_trip(&app, object_id, json!({})).await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-unit-away-with-trip", "entity": "object", "entity_uuid": object_uuid,
        "op": "set", "field": "counter_unit", "value": "h",
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "this object has trips; its counter must stay km or mi"
    );

    // km <-> mi stays allowed even with a live trip.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-unit-km-to-mi", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "counter_unit", "value": "mi",
            "edited_at": after_now(120), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let unit: Option<String> =
        sqlx::query_scalar("SELECT counter_unit FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(unit.as_deref(), Some("mi"));
}

/// `null` is the other way to leave `km`/`mi` besides switching to `h` (see the comment on the
/// check in `sync::apply::apply_op`, just above the one `has_trip` query both share) -- clearing
/// the counter entirely must be rejected the same way while a live trip depends on it, and
/// accepted once that trip is gone.
#[tokio::test]
async fn pushing_counter_unit_to_null_with_a_live_trip_is_rejected_until_the_trip_is_deleted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let object_id = bike["id"].as_i64().unwrap();
    let trip = create_trip(&app, object_id, json!({})).await;
    let trip_id = trip["id"].as_i64().unwrap();
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-unit-null-with-trip", "entity": "object", "entity_uuid": object_uuid,
        "op": "set", "field": "counter_unit", "value": null,
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "this object has trips; its counter must stay km or mi"
    );

    let res = app
        .client
        .delete(app.url(&format!("/activities/{trip_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204, "{}", res.text().await.unwrap());

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-unit-null-no-trip", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "counter_unit", "value": null,
            "edited_at": after_now(120), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let unit: Option<String> =
        sqlx::query_scalar("SELECT counter_unit FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        unit, None,
        "once the trip is gone, clearing the counter entirely is accepted"
    );
}
