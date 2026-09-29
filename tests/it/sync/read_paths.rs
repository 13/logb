use super::common;
use super::helpers::*;
use reqwest::multipart::{Form, Part};
use serde_json::json;

#[tokio::test]
async fn sync_cannot_mix_calendar_schedules_with_intervals() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Meter", Some("km")).await;
    let response = app.client.post(app.url(&format!("/objects/{}/reminders", object["id"])))
        .json(&json!({"title":"Reading", "kind":"reading", "every_n":1, "every_unit":"month"})).send().await.unwrap();
    assert_eq!(response.status(), 201);
    let reminder: serde_json::Value = response.json().await.unwrap();
    let response = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id":"calendar-conflict", "entity":"reminder", "entity_uuid":reminder["client_uuid"],
        "op":"set", "field":"schedule", "value":"daily", "edited_at":after_now(60), "device_id":"phone"
    }]))).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let result: serde_json::Value = response.json().await.unwrap();
    assert!(result.to_string().contains("rejected"), "{result}");
    let actual = app.get_json(&format!("/reminders/{}", reminder["id"])).await;
    assert!(actual["schedule"].is_null());
    assert_eq!(actual["every_n"], 1);
}

#[tokio::test]
async fn every_guarded_reminder_field_is_revalidated_on_sync() {
    // The guard in `apply_op` lists seven fields. A field named there but never pushed in a test
    // is a field whose validation can be dropped without anything failing, so each one gets a
    // value the REST handler refuses. The expected reason is asserted too: `every_n: 0` is turned
    // away by the generic numeric check before the reminder rules ever run, which would leave
    // this test passing while proving nothing about the guard.
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Meter", Some("km")).await;
    let response = app.client.post(app.url(&format!("/objects/{}/reminders", object["id"])))
        .json(&json!({"title":"Reading", "kind":"reading", "every_n":1, "every_unit":"month"})).send().await.unwrap();
    assert_eq!(response.status(), 201);
    let reminder: serde_json::Value = response.json().await.unwrap();
    let cases = [
        ("schedule", json!("daily"), "cannot be combined with an interval"),
        ("every_n", json!(61), "every_n must be 1..60"),
        ("every_unit", json!("fortnight"), "every_unit week or month"),
        ("repeat_months", json!(3), "not due_counter or repeat_*"),
        ("repeat_counter", json!(2), "not due_counter or repeat_*"),
        ("due_counter", json!(500), "not due_counter or repeat_*"),
        ("due_date", json!("2026-13-40"), "expected YYYY-MM-DD"),
    ];
    for (offset, (field, value, expected)) in cases.iter().enumerate() {
        let response = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": format!("guard-{field}"), "entity":"reminder", "entity_uuid":reminder["client_uuid"],
            "op":"set", "field":field, "value":value, "edited_at":after_now(60 + offset as i64), "device_id":"phone"
        }]))).send().await.unwrap();
        assert_eq!(response.status(), 200, "{field}");
        let result: serde_json::Value = response.json().await.unwrap();
        assert!(result.to_string().contains("rejected"), "{field} was accepted: {result}");
        let reason = result["results"][0]["reason"].as_str().unwrap_or_default();
        assert!(reason.contains(expected), "{field} was rejected for an unrelated reason: {reason}");
    }
    // Nothing above landed: the reminder is the one that was created.
    let actual = app.get_json(&format!("/reminders/{}", reminder["id"])).await;
    assert!(actual["schedule"].is_null());
    assert_eq!(actual["every_n"], 1);
    assert_eq!(actual["every_unit"], "month");
    assert!(actual["repeat_months"].is_null());
    assert!(actual["repeat_counter"].is_null());
    assert!(actual["due_counter"].is_null());
}

#[tokio::test]
async fn every_created_row_gets_a_client_uuid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(uuid.len(), 36, "a v4 uuid in hyphenated form: {uuid}");

    let deleted: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM objects WHERE id = $1")
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(deleted.is_none(), "a fresh row is not a tombstone");
}

#[tokio::test]
async fn the_log_tables_exist_and_start_empty() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let changes: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let clocks: i64 = sqlx::query_scalar("SELECT count(*) FROM field_clock")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!((changes, clocks), (0, 0));
}

#[tokio::test]
async fn deleting_an_object_tombstones_it_and_its_children() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();

    // The brief wrote this as `POST /activities` with an `object_id` in the body; the real
    // route hangs activities off their object, so it is spelled the way the API is.
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&serde_json::json!({
            "date": "2026-01-01", "category": "fuel",
            "title": "Fill-up", "notes": "", "counter_value": 1000, "cost_cents": 5000
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

    assert_eq!(
        app.client
            .delete(app.url(&format!("/objects/{object_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    let object_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(object_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(object_rows, 1, "the row survives; only deleted_at is set");

    let live: i64 =
        sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1 AND deleted_at IS NULL")
            .bind(object_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(live, 0, "the object is tombstoned");

    let live_children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activities WHERE object_id = $1 AND deleted_at IS NULL",
    )
    .bind(object_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(live_children, 0, "children are tombstoned with the parent");

    assert_eq!(
        app.client
            .get(app.url(&format!("/objects/{object_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        404,
        "a tombstoned object reads as absent"
    );
}


/// Checks all four of task 2's read-path filters (`/search`, `/export`, `/insights`, the
/// reminder digest) agree a deleted object's activity, reminder and attachment are gone, not
/// just the object itself.
///
/// This alone proves less than it looks like: every assertion below is satisfied by the
/// OBJECT-level filter alone (`o.deleted_at IS NULL`), because the fixture only ever deletes
/// the object -- a child-level filter (`a.deleted_at IS NULL`, `r.deleted_at IS NULL`,
/// `t.deleted_at IS NULL`) could be missing entirely from every one of these four read paths
/// and this test would still pass. `a_deleting_only_a_child_hides_it_from_every_read_path`
/// below is what actually exercises those, one child at a time.
#[tokio::test]
async fn a_deleted_objects_children_vanish_from_every_read_path() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Zyzzyva", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({
            "date": "2026-01-01", "category": "repair",
            "title": "Zyzzyva timing belt", "notes": "distinctive-note", "counter_value": 1000, "cost_cents": 5000
        }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let activity: serde_json::Value = res.json().await.unwrap();
    let activity_id = activity["id"].as_i64().unwrap();

    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/reminders")))
        .json(&json!({ "title": "Zyzzyva service", "due_date": "2020-01-01" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(
            Form::new()
                .part(
                    "file",
                    Part::bytes(png())
                        .file_name("belt.png")
                        .mime_str("image/png")
                        .unwrap(),
                )
                .text("activity_id", activity_id.to_string()),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let attachment: serde_json::Value = res.json().await.unwrap();
    let file_id = attachment["file_id"].as_i64().unwrap();

    // Sanity: everything above is visible before the delete, so what follows actually tests
    // the delete's effect and not an empty fixture.
    let r: serde_json::Value = app
        .client
        .get(app.url("/search?q=Zyzzyva"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["activities"].as_array().unwrap().len(), 1, "{r}");
    assert!(
        logb::notify::collect(&app.state).await.unwrap().is_some(),
        "the reminder really is due"
    );

    assert_eq!(
        app.client
            .delete(app.url(&format!("/objects/{object_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    // /search: neither the object nor its activity surface any more.
    let r: serde_json::Value = app
        .client
        .get(app.url("/search?q=Zyzzyva"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0, "{r}");
    assert_eq!(r["activities"].as_array().unwrap().len(), 0, "{r}");

    // /export: a full export walks live objects only, so a tombstoned one is skipped entirely.
    let res = app.client.get(app.url("/export")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let zip_bytes = res.bytes().await.unwrap().to_vec();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    let mut data_json = String::new();
    std::io::Read::read_to_string(&mut z.by_name("data.json").unwrap(), &mut data_json).unwrap();
    let data: serde_json::Value = serde_json::from_str(&data_json).unwrap();
    let names: Vec<&str> = data["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert!(
        !names.contains(&"Zyzzyva"),
        "a deleted object must not appear in a full export: {names:?}"
    );

    // /insights: the endpoint is scoped to one object id, and that object now reads as absent.
    let res = app
        .client
        .get(app.url(&format!("/objects/{object_id}/insights")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        404,
        "insights for a deleted object must 404, not roll up stale data"
    );

    // The reminder digest: the reminder was due before the delete, and must not be either.
    assert!(
        logb::notify::collect(&app.state).await.unwrap().is_none(),
        "a tombstoned reminder must not appear in the digest"
    );

    // The attachment's file, reachable only through it, is also unreadable.
    assert_eq!(
        app.client
            .get(app.url(&format!("/files/{file_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

/// The sibling above deletes the OBJECT, so every one of its assertions is satisfied by the
/// object-level filter (`o.deleted_at IS NULL`) alone -- a child-level filter could be missing
/// entirely from `search.rs`, `insights.rs`, `export.rs` or the reminder digest
/// (`api::reminders::due_for_user`) and that test would not notice. This one deletes a LIVE
/// object's activity, separately its reminder, and separately its attachment, with the object
/// itself untouched, and checks only the read paths that actually depend on the CHILD's own
/// `deleted_at`.
#[tokio::test]
async fn a_deleting_only_a_child_hides_it_from_every_read_path() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // -- An activity, deleted on its own. `/search` and `/objects/{id}/insights` both read
    // `activities.deleted_at` directly; neither has any reason to consult the object's.
    let car = app.create_object(&app.client, "Kwyjibo", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({
            "date": "2026-01-01", "category": "repair",
            "title": "Kwyjibo timing belt", "notes": "", "counter_value": 1000, "cost_cents": 5000
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let activity_id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    // Sanity: visible before the delete, so the assertions below test the delete's effect and
    // not an empty fixture.
    let r: serde_json::Value = app
        .client
        .get(app.url("/search?q=Kwyjibo"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["activities"].as_array().unwrap().len(), 1, "{r}");
    let insights: serde_json::Value = app
        .client
        .get(app.url(&format!("/objects/{object_id}/insights")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        insights["by_year"].as_array().unwrap().len(),
        1,
        "{insights}"
    );

    assert_eq!(
        app.client
            .delete(app.url(&format!("/activities/{activity_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    // The object itself is still very much alive -- only its child is gone.
    assert_eq!(
        app.client
            .get(app.url(&format!("/objects/{object_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    let r: serde_json::Value = app
        .client
        .get(app.url("/search?q=Kwyjibo"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        r["objects"].as_array().unwrap().len(),
        1,
        "the live object still matches: {r}"
    );
    assert_eq!(
        r["activities"].as_array().unwrap().len(),
        0,
        "a deleted activity must not surface: {r}"
    );

    let insights: serde_json::Value = app
        .client
        .get(app.url(&format!("/objects/{object_id}/insights")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        insights["by_year"].as_array().unwrap().len(),
        0,
        "a deleted activity must not roll into insights: {insights}"
    );
    assert_eq!(
        insights["by_category"].as_array().unwrap().len(),
        0,
        "{insights}"
    );

    let obj = export_object(&app, "Kwyjibo").await;
    assert_eq!(
        obj["activities"].as_array().unwrap().len(),
        0,
        "a deleted activity must not appear in export: {obj}"
    );

    // -- A reminder, deleted on its own. `/export` reads `reminders.deleted_at` directly, and
    // the reminder digest (`notify::collect`, via `due_for_user`) is built from the same
    // filter -- again, independently of whatever the object's own `deleted_at` says.
    let bike = app
        .create_object(&app.client, "Zyzzybalubah", Some("km"))
        .await;
    let bike_id = bike["id"].as_i64().unwrap();
    let res = app
        .client
        .post(app.url(&format!("/objects/{bike_id}/reminders")))
        .json(&json!({ "title": "Zyzzybalubah service", "due_date": "2020-01-01" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let reminder_id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    assert!(
        logb::notify::collect(&app.state).await.unwrap().is_some(),
        "sanity: the reminder really is due before the delete"
    );

    assert_eq!(
        app.client
            .delete(app.url(&format!("/reminders/{reminder_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    assert_eq!(
        app.client
            .get(app.url(&format!("/objects/{bike_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert!(
        logb::notify::collect(&app.state).await.unwrap().is_none(),
        "a deleted reminder must not appear in the digest, even though its object is alive"
    );

    let obj = export_object(&app, "Zyzzybalubah").await;
    assert_eq!(
        obj["reminders"].as_array().unwrap().len(),
        0,
        "a deleted reminder must not appear in export: {obj}"
    );

    // -- An attachment, deleted on its own. `GET /objects/{id}/attachments`, the activity-list
    // enrichment (`api::activities::with_attachments`) and `/export` all read
    // `attachments::for_object`, which filters `a.deleted_at IS NULL` -- again, independently
    // of whatever the object's own `deleted_at` says.
    let van = app.create_object(&app.client, "Bumpkis", Some("km")).await;
    let van_id = van["id"].as_i64().unwrap();
    let res = app
        .client
        .post(app.url(&format!("/objects/{van_id}/attachments")))
        .multipart(
            Form::new().part(
                "file",
                Part::bytes(png())
                    .file_name("bumpkis.png")
                    .mime_str("image/png")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    // Sanity: visible before the delete, so the assertions below test the delete's effect and
    // not an empty fixture.
    let atts: serde_json::Value = app
        .client
        .get(app.url(&format!("/objects/{van_id}/attachments")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(atts.as_array().unwrap().len(), 1, "{atts}");

    assert_eq!(
        app.client
            .delete(app.url(&format!("/attachments/{attachment_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    assert_eq!(
        app.client
            .get(app.url(&format!("/objects/{van_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    let atts: serde_json::Value = app
        .client
        .get(app.url(&format!("/objects/{van_id}/attachments")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        atts.as_array().unwrap().len(),
        0,
        "a deleted attachment must not surface: {atts}"
    );

    let obj = export_object(&app, "Bumpkis").await;
    assert_eq!(
        obj["attachments"].as_array().unwrap().len(),
        0,
        "a deleted attachment must not appear in export: {obj}"
    );
}

/// Fetches a full export and returns the one object named `name`, as a JSON value, so a test
/// can inspect its nested activities/reminders/attachments arrays.
async fn export_object(app: &common::TestApp, name: &str) -> serde_json::Value {
    let res = app.client.get(app.url("/export")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let zip_bytes = res.bytes().await.unwrap().to_vec();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    let mut data_json = String::new();
    std::io::Read::read_to_string(&mut z.by_name("data.json").unwrap(), &mut data_json).unwrap();
    let data: serde_json::Value = serde_json::from_str(&data_json).unwrap();
    data["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no object named {name} in the export: {data}"))
}
