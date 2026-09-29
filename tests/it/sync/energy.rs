use super::common;
use super::helpers::*;
use serde_json::json;


/// Creates a `fuel` activity, exactly as `POST .../activities` would, and returns its JSON.
async fn create_charge(
    app: &common::TestApp,
    object_id: i64,
    extra: serde_json::Value,
) -> serde_json::Value {
    // `fuel`, unlike `trip`, still requires a title -- `ActivityInput::validate`'s blank-title
    // exception is `trip`-only.
    let mut body = json!({
        "date": "2026-09-16", "category": "fuel", "title": "Charge", "notes": "",
        "counter_value": 3420, "quantity_milli": 8500, "cost_cents": 255
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

#[tokio::test]
async fn pushing_charged_full_on_a_fuel_row_is_accepted_and_stamps_the_field_clock() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let created = create_charge(&app, object_id, json!({})).await;
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-charged-full", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "charged_full", "value": 1,
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let stored: i64 =
        sqlx::query_scalar("SELECT charged_full FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, 1);

    let clocked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM field_clock WHERE entity = 'activity' AND entity_uuid = $1 AND field = 'charged_full'")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(
        clocked, 1,
        "the field clock must be stamped so a later, older write loses to this one"
    );
}

#[tokio::test]
async fn pushing_charged_full_on_a_non_fuel_row_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({
            "date": "2026-09-16", "category": "maintenance", "title": "Brakes", "notes": ""
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
            "client_op_id": "op-charged-full-non-fuel", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "charged_full", "value": 1,
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
        "only a charge can be marked full"
    );

    let stored: i64 =
        sqlx::query_scalar("SELECT charged_full FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, 0, "the rejected write must not land");
}

#[tokio::test]
async fn pushing_category_away_from_fuel_with_charged_full_stored_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let created = create_charge(&app, object_id, json!({ "charged_full": 1 })).await;
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-away-from-fuel-charged", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "category", "value": "maintenance",
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
        "only a charge can be marked full"
    );

    let category: String =
        sqlx::query_scalar("SELECT category FROM activities WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(category, "fuel", "the rejected write must not land");
}

/// The same category change is accepted once fuel-only fields are not in the way.
#[tokio::test]
async fn pushing_category_away_from_fuel_without_charged_full_is_accepted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let created = create_charge(&app, object_id, json!({})).await;
    let uuid = client_uuid(&app.state.db, "activities", created["id"].as_i64().unwrap()).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        {
            "client_op_id": "op-clear-fuel-quantity", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "quantity_milli", "value": null,
            "edited_at": after_now(60), "device_id": "phone"
        },
        {
            "client_op_id": "op-away-from-fuel-clean", "entity": "activity", "entity_uuid": uuid,
            "op": "set", "field": "category", "value": "maintenance",
            "edited_at": after_now(60), "device_id": "phone"
        }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    assert_eq!(body["results"][1]["outcome"], "accepted", "{body}");
}

#[tokio::test]
async fn pushing_energy_price_milli_with_a_stored_fuel_unit_is_accepted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app
        .client
        .post(app.url("/objects"))
        .json(&json!({
            "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "kwh",
            "description": "", "purchase_date": null, "purchase_price_cents": null
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let object_uuid = object_uuid(&app, car["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-price-with-unit", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "energy_price_milli", "value": 30000,
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let stored: Option<i64> =
        sqlx::query_scalar("SELECT energy_price_milli FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, Some(30000));
}

#[tokio::test]
async fn pushing_energy_price_milli_without_a_stored_fuel_unit_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_uuid = object_uuid(&app, car["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-price-no-unit", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "energy_price_milli", "value": 30000,
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
        "energy_price_milli needs a fuel unit"
    );

    let stored: Option<i64> =
        sqlx::query_scalar("SELECT energy_price_milli FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, None, "the rejected write must not land");
}

#[tokio::test]
async fn pushing_fuel_unit_to_null_while_a_price_is_stored_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app
        .client
        .post(app.url("/objects"))
        .json(&json!({
            "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "kwh",
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "energy_price_milli": 30000
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(car["energy_price_milli"], 30000);
    let object_uuid = object_uuid(&app, car["id"].as_i64().unwrap()).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-unit-null-with-price", "entity": "object", "entity_uuid": object_uuid,
        "op": "set", "field": "fuel_unit", "value": null,
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"],
        "energy_price_milli needs a fuel unit"
    );

    let unit: Option<String> =
        sqlx::query_scalar("SELECT fuel_unit FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        unit.as_deref(),
        Some("kwh"),
        "the rejected write must not land"
    );
}

/// The same field, accepted once no price stands in the way.
#[tokio::test]
async fn pushing_fuel_unit_to_null_without_a_stored_price_is_accepted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app
        .client
        .post(app.url("/objects"))
        .json(&json!({
            "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "kwh",
            "description": "", "purchase_date": null, "purchase_price_cents": null
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let object_uuid = object_uuid(&app, car["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-unit-null-no-price", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "fuel_unit", "value": null,
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
}

/// A pushed `fuel_unit` outside `l`/`gal`/`kwh` used to reach the row anyway with no cross-field
/// check standing in its way (only `Null` -- "clear it" -- is special-cased), relying entirely
/// on the CHECK constraint and its savepoint recovery to turn the write into a rejection with a
/// generic "violates a database constraint" reason instead of the specific one the REST door
/// gives for the identical mistake. `validate_value`'s new arm rejects it before either.
#[tokio::test]
async fn pushing_an_out_of_whitelist_fuel_unit_is_rejected_with_the_rest_wording() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_uuid = object_uuid(&app, car["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-fuel-unit-empty", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "fuel_unit", "value": "",
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
        "fuel_unit must be l, gal, kwh or null"
    );

    let stored: Option<String> =
        sqlx::query_scalar("SELECT fuel_unit FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, None, "the rejected write must not land");
}

/// A legal value on the same field still lands, the same door open on either side of the arm
/// added above.
#[tokio::test]
async fn pushing_a_whitelisted_fuel_unit_is_accepted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_uuid = object_uuid(&app, car["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-fuel-unit-kwh", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "fuel_unit", "value": "kwh",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let stored: Option<String> =
        sqlx::query_scalar("SELECT fuel_unit FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored.as_deref(), Some("kwh"));
}

/// `counter_unit` carries the identical CHECK-constraint-only hole `fuel_unit` had -- no
/// `validate_value` arm of its own, just the schema's CHECK and the savepoint recovery behind
/// it -- so it gets the same explicit arm and the same REST wording.
#[tokio::test]
async fn pushing_an_out_of_whitelist_counter_unit_is_rejected_with_the_rest_wording() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_uuid = object_uuid(&app, car["id"].as_i64().unwrap()).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-counter-unit-bad", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "counter_unit", "value": "furlongs",
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
        "counter_unit must be km, mi, h or null"
    );

    let stored: Option<String> =
        sqlx::query_scalar("SELECT counter_unit FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        stored.as_deref(),
        Some("km"),
        "the rejected write must not land"
    );
}
