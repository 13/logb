use super::common;
use super::helpers::*;
use reqwest::multipart::{Form, Part};
use serde_json::json;

/// A delete arriving over sync has to leave the same database behind as the same delete made
/// over REST. It did not: `apply_op` tombstoned only the row the op named, so an object deleted
/// through push kept live activities, reminders and attachments -- which the retention purge
/// then hard-deleted through the schema's `ON DELETE CASCADE`, without a tombstone and without
/// a log entry, so no other device ever learned they existed or vanished.

#[tokio::test]
async fn a_pushed_object_delete_cascades_tombstones_and_logs_each_child() {
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
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-del-object", "entity": "object", "entity_uuid": object_uuid,
            "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );

    for (table, id) in [
        ("activities", activity_id),
        ("reminders", reminder_id),
        ("attachments", attachment_id),
    ] {
        let deleted: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT deleted_at FROM {table} WHERE id = $1"
        )))
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        assert!(
            deleted.is_some(),
            "the {table} row must be tombstoned with its object"
        );
    }

    // A tombstone nobody is told about is the same as no tombstone at all for a device that
    // was offline, so each cascaded child has to reach the feed on its own uuid.
    for (entity, uuid) in [
        ("activity", &activity_uuid),
        ("reminder", &reminder_uuid),
        ("attachment", &attachment_uuid),
    ] {
        let logged: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM changes WHERE entity = $1 AND entity_uuid = $2 AND op = 'delete'",
        )
        .bind(entity)
        .bind(uuid)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        assert_eq!(
            logged, 1,
            "the cascaded {entity} delete must be in the log for other devices"
        );
    }

    let res = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    let uuids: Vec<&str> = body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["op"] == "delete")
        .map(|c| c["entity_uuid"].as_str().unwrap())
        .collect();
    for uuid in [
        &object_uuid,
        &activity_uuid,
        &reminder_uuid,
        &attachment_uuid,
    ] {
        assert!(
            uuids.contains(&uuid.as_str()),
            "pull must carry the delete of {uuid}: {uuids:?}"
        );
    }
}

/// The other half of the cascade `api::activities::delete` performs: an activity's attachments
/// go with it. Without this a pushed activity delete left attachments that only the retention
/// purge would ever remove, and it would remove them by destroying them.
#[tokio::test]
async fn a_pushed_activity_delete_cascades_to_its_attachments() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();

    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({
            "date": "2026-01-01", "category": "repair", "title": "Timing belt",
            "notes": "", "counter_value": 1000, "cost_cents": 5000
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        201,
        "create activity: {}",
        res.text().await.unwrap()
    );
    let activity_id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(
            Form::new()
                .text("activity_id", activity_id.to_string())
                .part(
                    "file",
                    Part::bytes(png())
                        .file_name("belt.png")
                        .mime_str("image/png")
                        .unwrap(),
                ),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        201,
        "create attachment: {}",
        res.text().await.unwrap()
    );
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;
    let attachment_uuid = client_uuid(&app.state.db, "attachments", attachment_id).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-del-activity", "entity": "activity", "entity_uuid": activity_uuid,
            "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );

    let deleted: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM attachments WHERE id = $1")
            .bind(attachment_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(
        deleted.is_some(),
        "the activity's attachment must be tombstoned with it"
    );

    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE entity = 'attachment' AND entity_uuid = $1 \
         AND op = 'delete'",
    )
    .bind(&attachment_uuid)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(
        logged, 1,
        "the cascaded attachment delete must be in the log"
    );
}

/// The one reference `c0de5be`'s cascade sweep missed: an attachment has no children of its
/// own, but it can be an object's cover, and that pointer is not a foreign key -- nothing but
/// `api::attachments::delete` clearing it by hand keeps it honest. A delete arriving over sync
/// skipped that, so the object was left naming a tombstoned (and eventually hard-deleted)
/// attachment id forever, flattened straight into `ObjectOut` and into every sync snapshot.
#[tokio::test]
async fn a_pushed_attachment_delete_clears_the_objects_cover() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, _activity_id, _reminder_id, attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;
    let attachment_uuid = client_uuid(&app.state.db, "attachments", attachment_id).await;

    // Make the attachment the object's cover, exactly as a client would before deleting it.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-set-cover", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "cover_attachment_id", "value": attachment_id,
            "edited_at": after_now(7776060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "set cover failed: {}",
        res.text().await.unwrap()
    );
    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE id = $1")
            .bind(object_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        cover,
        Some(attachment_id),
        "fixture setup: the cover must be set before deletion"
    );

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-attachment", "entity": "attachment", "entity_uuid": attachment_uuid,
        "op": "delete", "edited_at": after_now(7862460), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );

    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE id = $1")
            .bind(object_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(
        cover.is_none(),
        "a pushed attachment delete must clear the object's cover, same as REST"
    );
}

/// `apply_op`'s own-row tombstone `UPDATE` set only `deleted_at`, while the REST delete
/// handlers also stamp `updated_at`. Only `objects` and `activities` carry that column
/// (`reminders`, `attachments` and `files` do not -- see `migrations/sqlite/0001_init.sql`), so this
/// bumps the object's own row and, through `cascade_object`, the cascaded activity's row too.
#[tokio::test]
async fn a_pushed_object_delete_bumps_updated_at_on_itself_and_its_cascaded_activities() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, activity_id, _reminder_id, _attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;

    // Backdate both rows' `updated_at` so a later read can tell a real bump from a value that
    // was already current.
    let stale = "2020-01-01T00:00:00Z";
    sqlx::query("UPDATE objects SET updated_at = $1 WHERE id = $2")
        .bind(stale)
        .bind(object_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE activities SET updated_at = $1 WHERE id = $2")
        .bind(stale)
        .bind(activity_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-del-object", "entity": "object", "entity_uuid": object_uuid,
            "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );

    let object_updated: String = sqlx::query_scalar("SELECT updated_at FROM objects WHERE id = $1")
        .bind(object_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_ne!(
        object_updated, stale,
        "the object's own tombstone must bump updated_at"
    );

    let activity_updated: String =
        sqlx::query_scalar("SELECT updated_at FROM activities WHERE id = $1")
            .bind(activity_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_ne!(
        activity_updated, stale,
        "a cascaded activity tombstone must bump updated_at too"
    );
}

/// The other half of the own-row case above: an activity deleted directly (not cascaded from
/// its object) goes through `apply_op`'s own-row tombstone `UPDATE` rather than
/// `cascade_object`, and that branch has to bump `updated_at` too.
#[tokio::test]
async fn a_pushed_activity_delete_bumps_its_own_updated_at() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_object_id, activity_id, _reminder_id, _attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;

    let stale = "2020-01-01T00:00:00Z";
    sqlx::query("UPDATE activities SET updated_at = $1 WHERE id = $2")
        .bind(stale)
        .bind(activity_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-del-activity", "entity": "activity", "entity_uuid": activity_uuid,
            "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );

    let activity_updated: String =
        sqlx::query_scalar("SELECT updated_at FROM activities WHERE id = $1")
            .bind(activity_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_ne!(
        activity_updated, stale,
        "a directly-deleted activity must bump its own updated_at"
    );
}

/// The defect this pair of fixes exists for, end to end: a pushed delete followed by a purge
/// past the retention window must not destroy a single row silently, and must not strand a
/// blob on disk.
///
/// Before the fix the purge's `DELETE FROM objects` fired `ON DELETE CASCADE` over children
/// that were still live -- they vanished with no tombstone and no log entry, and the
/// attachment's `files` row became unreachable: `purge` builds its pinned list from TOMBSTONED
/// attachments, so a live one destroyed here is never a candidate and its bytes leak forever.
#[tokio::test]
async fn a_purge_after_a_pushed_delete_destroys_no_live_child_and_leaks_no_blob() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, activity_id, reminder_id, attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;

    let sha: String = sqlx::query_scalar(
        "SELECT f.sha256 FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = $1",
    )
    .bind(attachment_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let blob = app.state.storage.blob_path(&sha);
    assert!(blob.exists(), "the fixture's upload landed on disk");

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-del-object", "entity": "object", "entity_uuid": object_uuid,
            "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );

    // Only the object's tombstone is aged out. Its children were tombstoned just now, so they
    // are still inside the window and the purge has no business removing them yet -- through
    // the cascade or otherwise.
    sqlx::query("UPDATE objects SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = $1")
        .bind(object_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    logb::sync::feed::purge(&app.state, 90).await.unwrap();

    for (table, id) in [
        ("activities", activity_id),
        ("reminders", reminder_id),
        ("attachments", attachment_id),
    ] {
        let rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE id = $1"
        )))
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        assert_eq!(
            rows, 1,
            "the {table} row is still inside the window and must survive"
        );
    }
    assert!(
        blob.exists(),
        "no attachment has aged out, so its blob must still be pinned"
    );
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM files")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        files, 1,
        "the files row must not be orphaned by a destroyed attachment"
    );

    // And the wait is only a wait: once the children's own tombstones age out too, the purge
    // finishes the job and reclaims the bytes. Holding a parent back must not wedge it.
    for table in ["activities", "reminders", "attachments"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET deleted_at = '2000-01-01T00:00:00Z'"
        )))
        .execute(&app.state.db)
        .await
        .unwrap();
    }
    logb::sync::feed::purge(&app.state, 90).await.unwrap();

    for table in ["objects", "activities", "reminders", "attachments", "files"] {
        let rows: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&app.state.db)
                .await
                .unwrap();
        assert_eq!(
            rows, 0,
            "{table} should be empty once every tombstone has aged out"
        );
    }
    assert!(
        !blob.exists(),
        "the expired attachment finally frees the bytes"
    );
}

/// A `delete` op on `entity: "file"` used to fall into the same generic tombstone `UPDATE` as
/// every other entity. Nothing else in the codebase ever sets `files.deleted_at`: the purge's
/// `guards` array has no `files` entry (so the tombstone would never be purged), bootstrap
/// filters `deleted_at IS NULL` (so the file vanishes from every device's snapshot while its
/// still-live attachment keeps pointing at it), and the upload dedup has no `deleted_at` filter
/// (so re-uploading the same bytes would re-adopt the dead row and make the replacement photo
/// unrenderable too). One authenticated request could make an arbitrary number of photos
/// unrenderable, on every device, unrecoverable without hand-written SQL. The fix is to refuse
/// the op outright, so none of that machinery is ever exercised.
#[tokio::test]
async fn a_delete_op_on_a_file_is_rejected_and_leaves_it_live() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_object_id, _activity_id, _reminder_id, attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;

    let file_uuid: String = sqlx::query_scalar(
        "SELECT f.client_uuid FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = $1",
    )
    .bind(attachment_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-del-file", "entity": "file", "entity_uuid": file_uuid,
            "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");

    let deleted: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM files WHERE client_uuid = $1")
            .bind(&file_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(
        deleted.is_none(),
        "a file must never be tombstoned over sync"
    );

    let logged: i64 =
        sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-del-file'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(logged, 0, "a rejected op is not logged");
}

/// `apply_op` checks a value's shape against its column but, before this fix, nothing checked
/// the value ITSELF: only SQLite's own CHECK/NOT NULL constraints stood between a client and
/// the row, and most of these columns carry no such constraint. Each op below is something the
/// matching REST handler already rejects with 400 (`ObjectInput::validate`,
/// `ActivityInput::validate`, `ReminderInput::validate`); sync must refuse it identically.
#[tokio::test]
async fn a_value_the_rest_handlers_would_reject_is_also_rejected_over_sync() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, activity_id, reminder_id, _attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;
    let reminder_uuid = client_uuid(&app.state.db, "reminders", reminder_id).await;

    let cases = json!([
        { "client_op_id": "v-obj-name", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "name", "value": "   ",
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-obj-type", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "type", "value": "",
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-obj-date", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "purchase_date", "value": "not-a-date",
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-obj-price", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "purchase_price_cents", "value": -999,
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-act-date", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "date", "value": "not-a-date",
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-act-title", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "title", "value": "",
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-act-cost", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "cost_cents", "value": -1,
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-act-counter", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "counter_value", "value": -1,
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-act-qty", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "quantity_milli", "value": -1,
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-rem-title", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "title", "value": "",
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-rem-date", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "due_date", "value": "not-a-date",
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-rem-due-counter", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "due_counter", "value": -1,
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-rem-repeat-months", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "repeat_months", "value": 0,
          "edited_at": after_now(13046460), "device_id": "phone" },
        { "client_op_id": "v-rem-repeat-counter", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "repeat_counter", "value": 0,
          "edited_at": after_now(13046460), "device_id": "phone" }
    ]);
    let ids: Vec<String> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["client_op_id"].as_str().unwrap().to_string())
        .collect();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(cases))
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
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(
            body["results"][i]["outcome"], "rejected",
            "{id} must be rejected, same as the REST handler: {body}"
        );
    }

    // None of it landed: the object, activity and reminder still hold what the REST creates
    // put there.
    let (name, obj_type, purchase_date, price): (String, String, Option<String>, Option<i64>) =
        sqlx::query_as(
            "SELECT name, type, purchase_date, purchase_price_cents FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(
        (name.as_str(), obj_type.as_str(), purchase_date, price),
        ("Golf", "car", None, None)
    );

    let (date, title, cost, counter, qty): (String, String, Option<i64>, Option<i64>, Option<i64>) =
        sqlx::query_as(
            "SELECT date, title, cost_cents, counter_value, quantity_milli FROM activities WHERE client_uuid = $1")
            .bind(&activity_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(
        (date.as_str(), title.as_str(), cost, counter, qty),
        ("2026-01-01", "Timing belt", Some(5000), Some(1000), None)
    );

    let (rtitle, rdue_date, rdue_counter, rrepeat_months, rrepeat_counter):
        (String, Option<String>, Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT title, due_date, due_counter, repeat_months, repeat_counter FROM reminders WHERE client_uuid = $1")
        .bind(&reminder_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(
        (
            rtitle.as_str(),
            rdue_date.as_deref(),
            rdue_counter,
            rrepeat_months,
            rrepeat_counter
        ),
        ("Service", Some("2026-09-01"), None, None, None)
    );
}

/// The schema comments `field TEXT, -- NULL for create and delete`, but the push handler used
/// to bind `op.field`/`op.value` verbatim for every op kind, so junk carried on a `create` or
/// `delete` op was stored and served back over pull.
#[tokio::test]
async fn create_and_delete_ops_never_store_a_field_or_value() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_object_id, activity_id, _reminder_id, _attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "junk-create", "entity": "activity", "entity_uuid": activity_uuid,
              "op": "create", "field": "title", "value": "should not be stored",
              "edited_at": after_now(13132860), "device_id": "phone" },
            { "client_op_id": "junk-delete", "entity": "activity", "entity_uuid": activity_uuid,
              "op": "delete", "field": "title", "value": "should not be stored either",
              "edited_at": after_now(13132861), "device_id": "phone" }
        ])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    assert_eq!(body["results"][1]["outcome"], "accepted", "{body}");

    let rows: Vec<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT field, value FROM changes WHERE client_op_id IN ('junk-create', 'junk-delete') ORDER BY seq")
        .fetch_all(&app.state.db).await.unwrap();
    for (field, value) in rows {
        assert_eq!(
            (field, value),
            (None, None),
            "create and delete never carry a field or value"
        );
    }
}

/// `objects::update` refuses to point `cover_attachment_id` at anything but a live `photo`
/// attachment, but `cover_file_id` is derived from whatever `cover_attachment_id` names without
/// re-checking `kind`. The whitelist lets `kind` be changed over sync, so without a matching
/// guard there, `set attachment.kind = document` could turn a live cover into a document and
/// leave the pointer live but meaningless -- exactly what the REST guard exists to prevent.
#[tokio::test]
async fn changing_a_cover_attachments_kind_away_from_photo_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, _activity_id, _reminder_id, attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;
    let attachment_uuid = client_uuid(&app.state.db, "attachments", attachment_id).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-set-cover", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "cover_attachment_id", "value": attachment_id,
            "edited_at": after_now(13046460), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "set cover failed: {}",
        res.text().await.unwrap()
    );

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-kind-away", "entity": "attachment", "entity_uuid": attachment_uuid,
            "op": "set", "field": "kind", "value": "document",
            "edited_at": after_now(13132860), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");

    let kind: String = sqlx::query_scalar("SELECT kind FROM attachments WHERE id = $1")
        .bind(attachment_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        kind, "photo",
        "the cover's kind must not change while it is still the cover"
    );
    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE id = $1")
            .bind(object_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        cover,
        Some(attachment_id),
        "the cover pointer must be unaffected"
    );

    // Once it is no longer the cover, the same edit is an ordinary accepted write.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-clear-cover", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "cover_attachment_id", "value": null,
            "edited_at": after_now(13219260), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-kind-ok", "entity": "attachment", "entity_uuid": attachment_uuid,
            "op": "set", "field": "kind", "value": "document",
            "edited_at": after_now(13305660), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    let kind: String = sqlx::query_scalar("SELECT kind FROM attachments WHERE id = $1")
        .bind(attachment_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        kind, "document",
        "with no cover pinning it, kind is free to change"
    );
}
