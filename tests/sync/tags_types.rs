use super::common;
use super::helpers::*;
use serde_json::json;

/// Tags reach other devices both ways a device learns about a row: the bootstrap snapshot ships
/// the column verbatim (JSON text, like every other TEXT column), and a REST edit is logged as a
/// `set` whose value is that text, double-encoded like any string value (see
/// `a_pulled_change_rows_fields_match_the_op_that_produced_it`).
#[tokio::test]
async fn tags_travel_through_pull() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "car", "description": "", "tags": ["Lease", "winter"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/bootstrap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        body["objects"][0]["tags"], "[\"Lease\",\"winter\"]",
        "{body}"
    );

    let res = app.client.patch(app.url(&format!("/objects/{id}")))
        .json(&json!({ "name": "Golf", "type": "car", "description": "", "tags": ["Lease", "winter", "Tax"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let row = body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "set" && c["field"] == "tags")
        .unwrap_or_else(|| panic!("no set/tags row in {body}"));
    assert_eq!(
        row["value"],
        json!("[\"Lease\",\"winter\",\"Tax\"]").to_string()
    );
}

#[tokio::test]
async fn a_pushed_set_op_on_tags_is_normalised() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let uuid = object_uuid(&app, id).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-tags", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "tags", "value": "[\" Winter \",\"winter\",\"Lease\"]",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let obj = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(obj["tags"], json!(["Winter", "Lease"]));

    // Other devices must learn the stored tags, not the spelling this device happened to send.
    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let row = body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "set" && c["field"] == "tags")
        .unwrap_or_else(|| panic!("no set/tags row in {body}"));
    assert_eq!(row["value"], json!("[\"Winter\",\"Lease\"]").to_string());

    // Entries take the same field.
    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2026-03-01", "category": "repair", "title": "Tyres", "notes": "" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let activity_id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-act-tags", "entity": "activity", "entity_uuid": activity_uuid,
            "op": "set", "field": "tags", "value": "[\"tax 2026\",\"  TAX   2026 \"]",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"],
        "accepted"
    );
    let act = app.get_json(&format!("/activities/{activity_id}")).await;
    assert_eq!(act["tags"], json!(["tax 2026"]));
}

#[tokio::test]
async fn a_pushed_set_op_with_too_many_tags_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app
        .client
        .post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "car", "description": "", "tags": ["Lease"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();
    let uuid = object_uuid(&app, id).await;
    let before: serde_json::Value = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let since = before["next_seq"].as_i64().unwrap();
    let epoch = before["epoch"].as_str().unwrap().to_string();

    let many: Vec<String> = (0..11).map(|i| format!("t{i}")).collect();
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "op-many", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "tags", "value": serde_json::to_string(&many).unwrap(),
              "edited_at": after_now(60), "device_id": "phone" },
            { "client_op_id": "op-not-json", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "tags", "value": "Lease, winter",
              "edited_at": after_now(60), "device_id": "phone" }
        ])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "the batch must not 500: {}",
        res.text().await.unwrap()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert!(
        body["results"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("10"),
        "{body}"
    );
    assert_eq!(body["results"][1]["outcome"], "rejected", "{body}");

    let obj = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(obj["tags"], json!(["Lease"]));

    // A rejected op must not leak into the change feed either.
    let body: serde_json::Value = app
        .client
        .get(app.url(&format!("/sync/pull?since={since}&epoch={epoch}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Either as sent or double-encoded like any logged string value.
    let raw = [
        serde_json::to_string(&many).unwrap(),
        "Lease, winter".to_string(),
    ];
    let rejected: Vec<String> = raw
        .iter()
        .flat_map(|r| [r.clone(), json!(r).to_string()])
        .collect();
    let leaked = body["changes"].as_array().unwrap().iter().any(|c| {
        c["op"] == "set"
            && c["field"] == "tags"
            && c["value"]
                .as_str()
                .is_some_and(|v| rejected.iter().any(|r| r == v))
    });
    assert!(!leaked, "a rejected tags value reached the feed: {body}");
}

/// A pushed `create` of an object type, as a device that made the type offline sends it.
fn type_create_op(op_id: &str, uuid: &str, name: &str) -> serde_json::Value {
    json!({
        "client_op_id": op_id, "entity": "object_type", "entity_uuid": uuid, "op": "create",
        "value": { "name": name, "icon": "e-bike", "categories": ["repair", "fuel"], "counter_unit": "km" },
        "edited_at": after_now(60), "device_id": "phone"
    })
}

/// Ops apply in order inside one transaction, so an object can take a type the same push
/// created. The object row itself comes from REST, as every object does today (a sync `create`
/// only announces an existing row), so the object's side is its `set type`.
#[tokio::test]
async fn a_type_and_an_object_using_it_in_one_push() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let kick = app.create_object(&app.client, "Kick", Some("km")).await;
    let kick_uuid = object_uuid(&app, kick["id"].as_i64().unwrap()).await;
    let type_uuid = uuid::Uuid::new_v4().to_string();
    let key = format!("custom:{type_uuid}");

    let res = app.push_raw(&push_body(json!([
        type_create_op("op-type", &type_uuid, " E-scooter "),
        { "client_op_id": "op-object", "entity": "object", "entity_uuid": kick_uuid, "op": "create",
          "edited_at": after_now(61), "device_id": "phone" },
        { "client_op_id": "op-use", "entity": "object", "entity_uuid": kick_uuid, "op": "set",
          "field": "type", "value": key, "edited_at": after_now(62), "device_id": "phone" }
    ]))).await;
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    for i in 0..3 {
        assert_eq!(body["results"][i]["outcome"], "accepted", "{body}");
    }
    assert!(
        body["ids"][&type_uuid].is_i64(),
        "the push names the new type's id: {body}"
    );

    let snapshot = app.get_json("/sync/bootstrap").await;
    let types = snapshot["object_types"]
        .as_array()
        .unwrap_or_else(|| panic!("no object_types in {snapshot}"));
    assert_eq!(types.len(), 1, "{snapshot}");
    assert_eq!(types[0]["client_uuid"], type_uuid);
    assert_eq!(types[0]["name"], "E-scooter");
    assert_eq!(types[0]["categories"], "[\"repair\",\"fuel\",\"other\"]");
    assert_eq!(snapshot["objects"][0]["type"], key);
    assert_eq!(app.get_json("/types").await[0]["key"], key);

    // A replay of the create answers accepted without a second row, and a type key nobody made
    // is still refused.
    let res = app.push_raw(&push_body(json!([
        type_create_op("op-type-again", &type_uuid, "E-scooter"),
        { "client_op_id": "op-ghost", "entity": "object", "entity_uuid": kick_uuid, "op": "set",
          "field": "type", "value": format!("custom:{}", uuid::Uuid::new_v4()), "edited_at": after_now(63), "device_id": "phone" }
    ]))).await;
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    assert_eq!(body["results"][1]["outcome"], "rejected", "{body}");
    assert_eq!(app.get_json("/types").await.as_array().unwrap().len(), 1);

    // Another user can neither use the type nor create one under its uuid.
    let anna = app.create_user_client("anna", "password123").await;
    let res = anna
        .post(app.url("/sync/push"))
        .json(&push_body(json!([type_create_op(
            "anna-type",
            &type_uuid,
            "Mine"
        ),])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
}

#[tokio::test]
async fn a_set_op_renaming_a_type_is_validated() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let scooter = app
        .post_json(
            "/types",
            &json!({ "name": "E-scooter", "icon": "e-bike", "categories": ["repair"] }),
        )
        .await;
    app.post_json(
        "/types",
        &json!({ "name": "Boat", "icon": "tool", "categories": ["repair"] }),
    )
    .await;
    let uuid = scooter["client_uuid"].as_str().unwrap().to_string();
    let set = |op_id: &str, field: &str, value: serde_json::Value, secs: i64| {
        json!({
            "client_op_id": op_id, "entity": "object_type", "entity_uuid": uuid, "op": "set",
            "field": field, "value": value, "edited_at": after_now(secs), "device_id": "phone"
        })
    };

    let res = app
        .push_raw(&push_body(json!([
            set("rename", "name", json!("  Kickscooter "), 60),
            set("bad-icon", "icon", json!("rocket"), 61),
            set("taken", "name", json!("boat"), 62),
            set("cats", "categories", json!("[\"fuel\",\"fuel\"]"), 63),
            set("bad-cats", "categories", json!("[\"sailing\"]"), 64),
            set("no-name", "name", json!(null), 65),
            set("unit", "counter_unit", json!(null), 66),
        ])))
        .await;
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    let outcomes: Vec<&str> = body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(
        outcomes,
        ["accepted", "rejected", "rejected", "accepted", "rejected", "rejected", "accepted"],
        "{body}"
    );
    assert!(
        body["results"][1]["reason"]
            .as_str()
            .unwrap()
            .contains("icon"),
        "{body}"
    );

    let stored = app.get_json("/types").await;
    let stored = stored
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["client_uuid"] == uuid)
        .unwrap()
        .clone();
    assert_eq!(stored["name"], "Kickscooter");
    assert_eq!(stored["icon"], "e-bike");
    assert_eq!(stored["categories"], json!(["fuel", "other"]));
    assert_eq!(stored["counter_unit"], serde_json::Value::Null);

    // The feed carries what was stored, not the device's spelling.
    let feed = app.pull(0).await;
    let logged: Vec<(String, String)> = feed["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["entity"] == "object_type" && c["op"] == "set")
        .map(|c| {
            (
                c["field"].as_str().unwrap().to_string(),
                c["value"].as_str().unwrap_or("null").to_string(),
            )
        })
        .collect();
    assert!(
        logged.contains(&("name".into(), json!("Kickscooter").to_string())),
        "{feed}"
    );
    assert!(
        logged.contains(&(
            "categories".into(),
            json!("[\"fuel\",\"other\"]").to_string()
        )),
        "{feed}"
    );
    assert!(
        !logged.iter().any(|(f, _)| f == "icon"),
        "a rejected set must not reach the feed: {feed}"
    );

    // Another user's type is not theirs to rename.
    let anna = app.create_user_client("anna", "password123").await;
    let res = anna
        .post(app.url("/sync/push"))
        .json(&push_body(json!([set(
            "anna",
            "name",
            json!("Stolen"),
            70
        )])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
}

#[tokio::test]
async fn deleting_a_type_in_use_is_rejected_over_sync() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let scooter = app
        .post_json(
            "/types",
            &json!({ "name": "E-scooter", "icon": "e-bike", "categories": ["repair"] }),
        )
        .await;
    let uuid = scooter["client_uuid"].as_str().unwrap().to_string();
    let object = app
        .post_json(
            "/objects",
            &json!({ "name": "Kick", "type": scooter["key"] }),
        )
        .await;
    let delete = |op_id: &str, secs: i64| {
        push_body(json!([{
            "client_op_id": op_id, "entity": "object_type", "entity_uuid": uuid, "op": "delete",
            "edited_at": after_now(secs), "device_id": "phone"
        }]))
    };

    let body: serde_json::Value = app
        .push_raw(&delete("del-1", 60))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert!(
        body["results"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("in use"),
        "{body}"
    );
    assert_eq!(app.get_json("/types").await.as_array().unwrap().len(), 1);
    assert_eq!(app.count_changes_of("del-1").await, 0);

    app.delete_object(&object).await;
    let body: serde_json::Value = app
        .push_raw(&delete("del-2", 61))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    assert_eq!(app.get_json("/types").await, json!([]));
    let snapshot = app.get_json("/sync/bootstrap").await;
    assert_eq!(snapshot["object_types"], json!([]));
}

#[tokio::test]
async fn rest_type_writes_appear_in_the_change_feed() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let scooter = app
        .post_json(
            "/types",
            &json!({ "name": "E-scooter", "icon": "e-bike", "categories": ["repair"] }),
        )
        .await;
    let (id, uuid) = (
        scooter["id"].as_i64().unwrap(),
        scooter["client_uuid"].as_str().unwrap().to_string(),
    );
    let res = app.client.patch(app.url(&format!("/types/{id}")))
        .json(&json!({ "name": "Kickscooter", "icon": "e-bike", "categories": ["repair"], "counter_unit": "km" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);

    // The REST create/update stamped the clock, so a stale offline rename loses -- pushed while
    // the row is still live, because this is the only place left that proves a REST type write
    // stamps `field_clock` at all. Once the row is deleted below, the very same push would be
    // rejected for being on a deleted row before `field_clock` is ever read, and would no
    // longer exercise last-write-wins.
    let body: serde_json::Value = app
        .push_raw(&push_body(json!([{
            "client_op_id": "stale", "entity": "object_type", "entity_uuid": uuid, "op": "set",
            "field": "icon", "value": "tool", "edited_at": before_now(3600), "device_id": "phone"
        }])))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["results"][0]["outcome"], "superseded", "{body}");

    let res = app
        .client
        .delete(app.url(&format!("/types/{id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);

    let feed = app.pull(0).await;
    let rows: Vec<(String, Option<String>, Option<i64>)> = feed["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["entity"] == "object_type")
        .map(|c| {
            assert_eq!(c["entity_uuid"], uuid, "{feed}");
            (
                c["op"].as_str().unwrap().to_string(),
                c["field"].as_str().map(str::to_string),
                c["entity_id"].as_i64(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("create".to_string(), None, Some(id)),
            ("set".to_string(), Some("name".to_string()), Some(id)),
            (
                "set".to_string(),
                Some("counter_unit".to_string()),
                Some(id)
            ),
            // Unchanged by the PATCH above (same "e-bike" both times, so that write logged
            // nothing), but the superseded push above named it explicitly with a different value --
            // a superseded op is still part of the log, only a REJECTED one is not.
            ("set".to_string(), Some("icon".to_string()), Some(id)),
            ("delete".to_string(), None, Some(id)),
        ],
        "unchanged categories log nothing, and only a rejected op is absent from the log: {feed}"
    );

    // The type was deleted above, so the identical op shape is refused outright now -- it does
    // not even reach last-write-wins to lose there, since a `set` on a tombstoned row is
    // rejected before `field_clock` is ever read. The push above pins the ordinary LWW loss on
    // a still-live row; this pins the other rejection this same op shape can hit once the row
    // it names is gone.
    let body: serde_json::Value = app.push_raw(&push_body(json!([{
        "client_op_id": "stale-after-delete", "entity": "object_type", "entity_uuid": uuid, "op": "set",
        "field": "icon", "value": "tool", "edited_at": before_now(3600), "device_id": "phone"
    }]))).await.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"], "this item was deleted",
        "{body}"
    );
}
