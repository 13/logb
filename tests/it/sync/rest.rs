use super::common;
use super::helpers::*;
use reqwest::multipart::{Form, Part};
use serde_json::json;


// -- Task 9: REST writes recorded in the sync log ---------------------------------------

/// The reason this task exists. Before it, a REST write left `field_clock` untouched, so a
/// browser edit had no clock entry to protect it: a phone `set` op arriving later, but
/// carrying an OLDER `edited_at` than the browser's edit, still found no stored `field_clock`
/// row to lose to and was accepted -- silently overwriting the newer browser value. Once the
/// REST update stamps `field_clock` (in the canonical form `wins` compares -- rule 1), the
/// same stale phone op instead loses last-write-wins and comes back `superseded`.
///
/// A REST write never checks `wins` itself -- it always describes what a live user just did,
/// so it always applies and always re-stamps the clock at the real current instant (see
/// `sync::record::record_update`). The first phone op below is dated safely in the future
/// (`after_now`) purely so it clears the clock `record_create` already stamped at real "now"
/// when the object was made, and is accepted -- establishing that the browser's later
/// overwrite really is competing against a *newer*-looking stamp, not an empty one. The second
/// phone op is dated safely in the past (`before_now`), standing in for "before the browser's
/// edit" without depending on wall-clock timing precision.
#[tokio::test]
async fn a_rest_edit_beats_a_sync_op_stamped_before_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "phone-op-1", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Phone Golf",
            "edited_at": after_now(31536060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    assert_eq!(
        res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"],
        "accepted"
    );

    // The browser edits the same field over REST. Every other field is resent unchanged so
    // only `name` moves. This unconditionally overwrites `field_clock` to the real current
    // instant, regardless of the phone's on-paper-later stamp -- a REST write always reflects
    // what just happened.
    let res = app
        .client
        .patch(app.url(&format!("/objects/{id}")))
        .json(&json!({
            "name": "Browser Golf", "type": "car", "counter_unit": "km", "description": "",
            "purchase_date": null, "purchase_price_cents": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "phone-op-2", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Late Phone Golf",
            "edited_at": before_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "superseded", "{body}");

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        name, "Browser Golf",
        "the REST edit must survive a sync op stamped before it"
    );
}

/// A browser create, edit and delete of the same object must each appear on `GET /sync/pull`,
/// with the entity/op/field the write actually performed.
#[tokio::test]
async fn a_browser_create_edit_and_delete_are_all_visible_on_pull() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let res = app
        .client
        .patch(app.url(&format!("/objects/{id}")))
        .json(&json!({
            "name": "Golf VII", "type": "car", "counter_unit": "km", "description": "",
            "purchase_date": null, "purchase_price_cents": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    assert_eq!(
        app.client
            .delete(app.url(&format!("/objects/{id}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    let res = app.client.get(app.url("/sync/pull")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    let mine: Vec<serde_json::Value> = body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["entity_uuid"] == uuid)
        .cloned()
        .collect();
    assert_eq!(mine.len(), 3, "create, one set, delete: {mine:?}");

    assert_eq!(mine[0]["entity"], "object");
    assert_eq!(mine[0]["op"], "create");
    assert!(mine[0]["field"].is_null());

    assert_eq!(mine[1]["op"], "set");
    assert_eq!(mine[1]["field"], "name");
    assert_eq!(mine[1]["value"], "\"Golf VII\"");

    assert_eq!(mine[2]["op"], "delete");
    assert!(mine[2]["field"].is_null());
}

/// The log records what happened, and rewriting a field with the value it already holds did
/// not happen -- see `sync::record::record_update`.
#[tokio::test]
async fn rewriting_a_field_with_its_existing_value_logs_nothing() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(before, 1, "the create itself is logged");

    let res = app
        .client
        .patch(app.url(&format!("/objects/{id}")))
        .json(&json!({
            "name": "Golf", "type": "car", "counter_unit": "km", "description": "",
            "purchase_date": null, "purchase_price_cents": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(after, before, "a no-op edit must not add to the log");
}

/// The delete cascade is shared code (`sync::record::cascade_object`), not two hand-rolled
/// copies -- a REST delete of an object must log a `delete` for the object itself and for
/// every child the cascade tombstones, exactly as a sync delete does.
#[tokio::test]
async fn a_browser_delete_logs_the_object_and_every_cascaded_child() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, activity_id, reminder_id, attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;
    let reminder_uuid = client_uuid(&app.state.db, "reminders", reminder_id).await;
    let attachment_uuid = client_uuid(&app.state.db, "attachments", attachment_id).await;

    let res = app
        .client
        .delete(app.url(&format!("/objects/{object_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);

    for (uuid, label) in [
        (&object_uuid, "object"),
        (&activity_uuid, "activity"),
        (&reminder_uuid, "reminder"),
        (&attachment_uuid, "attachment"),
    ] {
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT op, field FROM changes WHERE entity_uuid = $1 AND op = 'delete'",
        )
        .bind(uuid)
        .fetch_optional(&app.state.db)
        .await
        .unwrap();
        let (op, field) = row.unwrap_or_else(|| panic!("no delete logged for {label} ({uuid})"));
        assert_eq!(op, "delete", "{label}");
        assert!(
            field.is_none(),
            "{label}: create and delete never carry a field"
        );
    }
}

/// Settings and user rows are not part of the sync protocol -- `sync::whitelist` only ever
/// names object/activity/reminder/attachment/file -- so writing them must never touch
/// `changes`, however the write happened.
#[tokio::test]
async fn settings_and_user_writes_produce_no_changes_rows() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let res = app
        .client
        .put(app.url("/settings"))
        .json(&json!({ "currency": "USD" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let _ = app.create_user_client("mallory", "correct horse").await;

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "settings and user writes are outside the sync protocol"
    );
}



/// The sync door must refuse a cycle exactly as the REST door does -- proving the two doors
/// agree, not just that each one independently rejects something. The legitimate reparenting
/// is pushed first, so a blanket "parent_id is not settable" cannot pass this test by
/// rejecting everything.
#[tokio::test]
async fn a_sync_push_cannot_create_a_cycle() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let house_uuid = client_uuid(&app.state.db, "objects", house["id"].as_i64().unwrap()).await;
    let garage_uuid = client_uuid(&app.state.db, "objects", garage["id"].as_i64().unwrap()).await;

    // A sync op naming another row carries that row's real integer id -- the same convention
    // `cover_attachment_id` already uses. Garage becomes House's child.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "op-nest", "entity": "object", "entity_uuid": garage_uuid,
              "op": "set", "field": "parent_id", "value": house["id"].as_i64().unwrap(),
              "edited_at": after_now(60), "device_id": "phone" }
        ])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        body["results"][0]["outcome"], "accepted",
        "a legitimate parent must land: {body}"
    );

    // Now a device tries to push the reverse: House becomes Garage's child.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "op-cycle", "entity": "object", "entity_uuid": house_uuid,
              "op": "set", "field": "parent_id", "value": garage["id"].as_i64().unwrap(),
              "edited_at": after_now(60), "device_id": "phone" }
        ])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "the batch must not 500");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        body["results"][0]["outcome"], "rejected",
        "a cycle must be rejected, not applied"
    );

    let parent: Option<i64> =
        sqlx::query_scalar("SELECT parent_id FROM objects WHERE client_uuid = $1")
            .bind(&house_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(parent.is_none(), "the cycle must not have landed");
}

#[tokio::test]
async fn every_pulled_change_names_the_servers_id_for_its_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let activity: serde_json::Value = app
        .client
        .post(app.url(&format!("/objects/{}/activities", car["id"])))
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "Wipers" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // A create logs only a `create` row; an edit is what produces a `set` row to check.
    let res = app
        .client
        .patch(app.url(&format!("/activities/{}", activity["id"])))
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "Wiper blades" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    let pulled: serde_json::Value = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let changes = pulled["changes"].as_array().unwrap();
    let object_create = changes
        .iter()
        .find(|c| c["entity"] == "object" && c["op"] == "create")
        .unwrap();
    assert_eq!(object_create["entity_id"], car["id"]);
    let activity_create = changes
        .iter()
        .find(|c| c["entity"] == "activity" && c["op"] == "create")
        .unwrap();
    assert_eq!(activity_create["entity_id"], activity["id"]);
    // A set row names the same id as the create for the same uuid.
    let activity_set = changes
        .iter()
        .find(|c| c["entity"] == "activity" && c["op"] == "set")
        .unwrap();
    assert_eq!(activity_set["entity_id"], activity["id"]);
}

#[tokio::test]
async fn deleting_a_cover_attachment_logs_the_cover_being_cleared() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let a: serde_json::Value = app
        .client
        .post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(
            Form::new().part(
                "file",
                Part::bytes(png())
                    .file_name("a.png")
                    .mime_str("image/png")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Make it the cover, then delete it.
    let res = app.client.patch(app.url(&format!("/objects/{id}")))
        .json(&json!({ "name": "Golf", "type": "car", "counter_unit": "km", "cover_attachment_id": a["id"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
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
    let res = app
        .client
        .delete(app.url(&format!("/attachments/{}", a["id"])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);

    let after: serde_json::Value = app
        .client
        .get(app.url(&format!("/sync/pull?since={since}&epoch={epoch}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let cleared = after["changes"].as_array().unwrap().iter().find(|c| {
        c["entity"] == "object" && c["op"] == "set" && c["field"] == "cover_attachment_id"
    });
    let cleared = cleared.expect("the cover clear is in the feed");
    assert_eq!(cleared["entity_id"], id);
    // A REST-side write stores the double-encoded JSON `null` (the string "null"), while a
    // pushed op with an explicit null stores SQL NULL; a client must read both as "clear".
    assert!(
        cleared["value"].is_null() || cleared["value"] == "null",
        "value clears the field: {:?}",
        cleared["value"]
    );
}

#[tokio::test]
async fn deleting_a_done_activity_logs_the_reminder_being_unlinked() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let oid = car["id"].as_i64().unwrap();
    let activity: serde_json::Value = app
        .client
        .post(app.url(&format!("/objects/{oid}/activities")))
        .json(&json!({ "date": "2026-09-01", "category": "maintenance", "title": "Oil" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let reminder: serde_json::Value = app
        .client
        .post(app.url(&format!("/objects/{oid}/reminders")))
        .json(&json!({ "title": "Oil", "due_date": "2026-09-01" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let res = app
        .client
        .post(app.url(&format!("/reminders/{}/done", reminder["id"])))
        .json(&json!({ "activity_id": activity["id"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
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
    let res = app
        .client
        .delete(app.url(&format!("/activities/{}", activity["id"])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);

    let after: serde_json::Value = app
        .client
        .get(app.url(&format!("/sync/pull?since={since}&epoch={epoch}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let unlinked = after["changes"].as_array().unwrap().iter().find(|c| {
        c["entity"] == "reminder" && c["op"] == "set" && c["field"] == "done_activity_id"
    });
    assert!(unlinked.is_some(), "the unlink is in the feed");
    assert_eq!(unlinked.unwrap()["entity_id"], reminder["id"]);
}
