mod common;
use reqwest::multipart::{Form, Part};
use serde_json::json;

/// An `edited_at` a fixed number of seconds after "right now", in the same canonical form
/// `sync::apply::canonical_edited_at` produces. A literal calendar date pinned "in the future"
/// (2031, say) is only in the future until the wall clock passes it -- this file used to
/// hardcode exactly that, and every `set` op relying on it to beat a REST create's real-time
/// `field_clock` stamp would have started failing the moment "now" caught up to the literal.
/// An offset from the clock the test actually runs against cannot expire.
fn after_now(secs: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(secs))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The mirror of `after_now`, for an op that must lose last-write-wins against something
/// stamped at real "now" -- e.g. a phone op standing in for "before the browser's edit".
fn before_now(secs: i64) -> String {
    after_now(-secs)
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

    let deleted: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM objects WHERE id = $1")
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
        .fetch_one(&app.state.db).await.unwrap();
    let clocks: i64 = sqlx::query_scalar("SELECT count(*) FROM field_clock")
        .fetch_one(&app.state.db).await.unwrap();
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
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities"))).json(&serde_json::json!({
        "date": "2026-01-01", "category": "fuel",
        "title": "Fill-up", "notes": "", "counter_value": 1000, "cost_cents": 5000
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "create activity: {}", res.text().await.unwrap());

    assert_eq!(
        app.client.delete(app.url(&format!("/objects/{object_id}"))).send().await.unwrap().status(),
        204
    );

    let object_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(object_rows, 1, "the row survives; only deleted_at is set");

    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM objects WHERE id = $1 AND deleted_at IS NULL")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(live, 0, "the object is tombstoned");

    let live_children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activities WHERE object_id = $1 AND deleted_at IS NULL")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(live_children, 0, "children are tombstoned with the parent");

    assert_eq!(
        app.client.get(app.url(&format!("/objects/{object_id}"))).send().await.unwrap().status(),
        404,
        "a tombstoned object reads as absent"
    );
}

fn png() -> Vec<u8> {
    let img = image::DynamicImage::new_rgb8(8, 8);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
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

    let res = app.client.post(app.url(&format!("/objects/{object_id}/reminders")))
        .json(&json!({ "title": "Zyzzyva service", "due_date": "2020-01-01" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    let res = app.client.post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(
            Form::new()
                .part("file", Part::bytes(png()).file_name("belt.png").mime_str("image/png").unwrap())
                .text("activity_id", activity_id.to_string()),
        )
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let attachment: serde_json::Value = res.json().await.unwrap();
    let file_id = attachment["file_id"].as_i64().unwrap();

    // Sanity: everything above is visible before the delete, so what follows actually tests
    // the delete's effect and not an empty fixture.
    let r: serde_json::Value = app.client.get(app.url("/search?q=Zyzzyva")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["activities"].as_array().unwrap().len(), 1, "{r}");
    assert!(logb::notify::collect(&app.state).await.unwrap().is_some(), "the reminder really is due");

    assert_eq!(
        app.client.delete(app.url(&format!("/objects/{object_id}"))).send().await.unwrap().status(),
        204
    );

    // /search: neither the object nor its activity surface any more.
    let r: serde_json::Value = app.client.get(app.url("/search?q=Zyzzyva")).send().await.unwrap().json().await.unwrap();
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
    let names: Vec<&str> = data["objects"].as_array().unwrap().iter().map(|o| o["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&"Zyzzyva"), "a deleted object must not appear in a full export: {names:?}");

    // /insights: the endpoint is scoped to one object id, and that object now reads as absent.
    let res = app.client.get(app.url(&format!("/objects/{object_id}/insights"))).send().await.unwrap();
    assert_eq!(res.status(), 404, "insights for a deleted object must 404, not roll up stale data");

    // The reminder digest: the reminder was due before the delete, and must not be either.
    assert!(
        logb::notify::collect(&app.state).await.unwrap().is_none(),
        "a tombstoned reminder must not appear in the digest"
    );

    // The attachment's file, reachable only through it, is also unreadable.
    assert_eq!(app.client.get(app.url(&format!("/files/{file_id}"))).send().await.unwrap().status(), 404);
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
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({
            "date": "2026-01-01", "category": "repair",
            "title": "Kwyjibo timing belt", "notes": "", "counter_value": 1000, "cost_cents": 5000
        }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let activity_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    // Sanity: visible before the delete, so the assertions below test the delete's effect and
    // not an empty fixture.
    let r: serde_json::Value =
        app.client.get(app.url("/search?q=Kwyjibo")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["activities"].as_array().unwrap().len(), 1, "{r}");
    let insights: serde_json::Value = app.client.get(app.url(&format!("/objects/{object_id}/insights")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(insights["by_year"].as_array().unwrap().len(), 1, "{insights}");

    assert_eq!(
        app.client.delete(app.url(&format!("/activities/{activity_id}"))).send().await.unwrap().status(),
        204
    );

    // The object itself is still very much alive -- only its child is gone.
    assert_eq!(app.client.get(app.url(&format!("/objects/{object_id}"))).send().await.unwrap().status(), 200);

    let r: serde_json::Value =
        app.client.get(app.url("/search?q=Kwyjibo")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "the live object still matches: {r}");
    assert_eq!(r["activities"].as_array().unwrap().len(), 0, "a deleted activity must not surface: {r}");

    let insights: serde_json::Value = app.client.get(app.url(&format!("/objects/{object_id}/insights")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(insights["by_year"].as_array().unwrap().len(), 0, "a deleted activity must not roll into insights: {insights}");
    assert_eq!(insights["by_category"].as_array().unwrap().len(), 0, "{insights}");

    let obj = export_object(&app, "Kwyjibo").await;
    assert_eq!(obj["activities"].as_array().unwrap().len(), 0, "a deleted activity must not appear in export: {obj}");

    // -- A reminder, deleted on its own. `/export` reads `reminders.deleted_at` directly, and
    // the reminder digest (`notify::collect`, via `due_for_user`) is built from the same
    // filter -- again, independently of whatever the object's own `deleted_at` says.
    let bike = app.create_object(&app.client, "Zyzzybalubah", Some("km")).await;
    let bike_id = bike["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{bike_id}/reminders")))
        .json(&json!({ "title": "Zyzzybalubah service", "due_date": "2020-01-01" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let reminder_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    assert!(
        logb::notify::collect(&app.state).await.unwrap().is_some(),
        "sanity: the reminder really is due before the delete"
    );

    assert_eq!(
        app.client.delete(app.url(&format!("/reminders/{reminder_id}"))).send().await.unwrap().status(),
        204
    );

    assert_eq!(app.client.get(app.url(&format!("/objects/{bike_id}"))).send().await.unwrap().status(), 200);
    assert!(
        logb::notify::collect(&app.state).await.unwrap().is_none(),
        "a deleted reminder must not appear in the digest, even though its object is alive"
    );

    let obj = export_object(&app, "Zyzzybalubah").await;
    assert_eq!(obj["reminders"].as_array().unwrap().len(), 0, "a deleted reminder must not appear in export: {obj}");

    // -- An attachment, deleted on its own. `GET /objects/{id}/attachments`, the activity-list
    // enrichment (`api::activities::with_attachments`) and `/export` all read
    // `attachments::for_object`, which filters `a.deleted_at IS NULL` -- again, independently
    // of whatever the object's own `deleted_at` says.
    let van = app.create_object(&app.client, "Bumpkis", Some("km")).await;
    let van_id = van["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{van_id}/attachments")))
        .multipart(
            Form::new()
                .part("file", Part::bytes(png()).file_name("bumpkis.png").mime_str("image/png").unwrap()),
        )
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    // Sanity: visible before the delete, so the assertions below test the delete's effect and
    // not an empty fixture.
    let atts: serde_json::Value = app.client.get(app.url(&format!("/objects/{van_id}/attachments")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(atts.as_array().unwrap().len(), 1, "{atts}");

    assert_eq!(
        app.client.delete(app.url(&format!("/attachments/{attachment_id}"))).send().await.unwrap().status(),
        204
    );

    assert_eq!(app.client.get(app.url(&format!("/objects/{van_id}"))).send().await.unwrap().status(), 200);

    let atts: serde_json::Value = app.client.get(app.url(&format!("/objects/{van_id}/attachments")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(atts.as_array().unwrap().len(), 0, "a deleted attachment must not surface: {atts}");

    let obj = export_object(&app, "Bumpkis").await;
    assert_eq!(obj["attachments"].as_array().unwrap().len(), 0, "a deleted attachment must not appear in export: {obj}");
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
    data["objects"].as_array().unwrap().iter().find(|o| o["name"] == name).cloned()
        .unwrap_or_else(|| panic!("no object named {name} in the export: {data}"))
}

/// An op batch as the wire format expects it.
fn push_body(ops: serde_json::Value) -> serde_json::Value {
    json!({ "ops": ops })
}

#[tokio::test]
async fn a_set_op_updates_the_row_and_is_logged() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-1", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Golf VII",
        "edited_at": after_now(2714460), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted");
    assert!(body["server_time"].is_string());

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Golf VII");

    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-1'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1);
}

#[tokio::test]
async fn an_older_edit_is_superseded_but_still_recorded() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let newer = json!([{
        "client_op_id": "op-new", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Newer",
        "edited_at": after_now(2764860), "device_id": "phone"
    }]);
    let older = json!([{
        "client_op_id": "op-old", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Older",
        "edited_at": after_now(2678460), "device_id": "phone"
    }]);
    app.client.post(app.url("/sync/push")).json(&push_body(newer)).send().await.unwrap();
    let res = app.client.post(app.url("/sync/push")).json(&push_body(older)).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();

    assert_eq!(body["results"][0]["outcome"], "superseded");
    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Newer", "the loser must not overwrite the winner");
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-old'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1, "a superseded op is still part of the log");
}

#[tokio::test]
async fn a_replayed_push_is_idempotent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let batch = push_body(json!([{
        "client_op_id": "op-same", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Once",
        "edited_at": after_now(2678460), "device_id": "phone"
    }]));
    // Both attempts must answer `accepted`: the first because it genuinely applied, the
    // second because idempotency reports the earlier attempt's outcome, not a fresh
    // `superseded` from replaying against the clock its own first attempt just stamped.
    // Asserting only the log count below would pass just the same if the first attempt had
    // silently lost last-write-wins -- a `changes` row is written for `superseded` too (see
    // `an_older_edit_is_superseded_but_still_recorded`), so a log count of 1 alone does not
    // prove this op ever actually applied.
    for _ in 0..2 {
        let res = app.client.post(app.url("/sync/push")).json(&batch).send().await.unwrap();
        assert_eq!(res.status(), 200);
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    }
    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Once", "the op actually applied, not merely logged");

    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-same'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1, "the same op id lands exactly once");
}

#[tokio::test]
async fn timestamps_are_compared_chronologically_not_lexically() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    // An anchor safely in the future, floored to a whole second so "whole" and "frac" below
    // differ by exactly 500ms with nothing left to chance from `Utc::now()`'s own fraction.
    let anchor = chrono::DateTime::<chrono::Utc>::from_timestamp(
        (chrono::Utc::now() + chrono::Duration::seconds(600)).timestamp(), 0,
    ).unwrap();
    // No fraction at all -- what a client that never bothered with sub-second precision sends.
    let whole = anchor.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    // Half a second LATER, written with a fraction. Compared as raw strings the fractional one
    // loses ('.' < 'Z'), so a lexical rule would keep "Early"; canonicalization is what stops
    // that.
    let frac = (anchor + chrono::Duration::milliseconds(500))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    for (id, value, at) in [("op-whole", "Early", whole.as_str()), ("op-frac", "Later", frac.as_str())] {
        let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": id, "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": value,
            "edited_at": at, "device_id": "phone"
        }]))).send().await.unwrap();
        assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    }

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Later", "the chronologically later edit must win");

    // The same instant as `whole`, in offset-notation form (`+00:00` rather than `Z`); must
    // not re-win over the "Later" (`frac`) value it is chronologically earlier than.
    let offset_form = anchor.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-offset", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Earlier still",
        "edited_at": offset_form, "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "superseded");

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-junk", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Nonsense",
        "edited_at": "last thursday", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "rejected");

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Later");
}

#[tokio::test]
async fn a_field_outside_the_whitelist_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-evil", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "user_id", "value": 2,
        "edited_at": after_now(2678460), "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");
}

/// A device that was offline across the upgrade arrives with an operation naming a column that
/// no longer exists. It must be rejected on its own -- a batch that 500s is retried identically
/// forever, which is how one bad operation once wedged a client permanently.
#[tokio::test]
async fn an_operation_naming_the_removed_field_is_rejected_not_fatal() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "op-1", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "category", "value": "auto",
          "edited_at": after_now(60), "device_id": "phone" },
        { "client_op_id": "op-2", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "name", "value": "Golf VII",
          "edited_at": after_now(60), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "one bad op must not fail the batch: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");
    assert_eq!(body["results"][1]["outcome"], "accepted", "the good op in the same batch applies");
}

#[tokio::test]
async fn a_foreign_key_field_cannot_point_at_another_users_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let victim_object = car["id"].as_i64().unwrap();
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"%PDF-1.4 fake".to_vec())
            .file_name("invoice.pdf")
            .mime_str("application/pdf")
            .unwrap(),
    );
    let res = app.client.post(app.url(&format!("/objects/{victim_object}/attachments")))
        .multipart(form).send().await.unwrap();
    assert_eq!(res.status(), 201, "upload failed: {}", res.text().await.unwrap());
    let victim_attachment = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    // A second account, with an object of its own, tries to adopt the first account's
    // attachment as its cover image.
    let mallory = app.create_user_client("mallory", "another password").await;
    let theirs = app.create_object(&mallory, "Bike", None).await;
    let their_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(theirs["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = mallory.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-steal", "entity": "object", "entity_uuid": their_uuid,
        "op": "set", "field": "cover_attachment_id", "value": victim_attachment,
        "edited_at": after_now(15638460), "device_id": "mallory-phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");

    let cover: Option<i64> = sqlx::query_scalar(
        "SELECT cover_attachment_id FROM objects WHERE client_uuid = $1")
        .bind(&their_uuid).fetch_one(&app.state.db).await.unwrap();
    assert!(cover.is_none(), "the cross-account reference must not have landed");
}

#[tokio::test]
async fn one_user_cannot_push_at_another_users_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let other = app.create_user_client("mallory", "another password").await;
    let res = other.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-cross", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Stolen",
        "edited_at": "2030-01-01T00:00:00Z", "device_id": "mallory-phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Golf", "an unrelated user changed nothing");
}

#[tokio::test]
async fn results_stay_in_the_order_the_ops_were_sent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    // A batch that mixes ops rejected at different stages -- an unparseable timestamp, a field
    // off the whitelist -- with ones that land. `results[i]` must still describe `ops[i]`, so a
    // client can line the two lists up by position and not only by `client_op_id`.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "ord-1", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "name", "value": "First",
          "edited_at": after_now(5097660), "device_id": "phone" },
        { "client_op_id": "ord-2", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "category", "value": "car",
          "edited_at": "not a timestamp", "device_id": "phone" },
        { "client_op_id": "ord-3", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "description", "value": "Mine",
          "edited_at": after_now(5097660), "device_id": "phone" },
        { "client_op_id": "ord-4", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "user_id", "value": 2,
          "edited_at": after_now(5097660), "device_id": "phone" },
        { "client_op_id": "ord-5", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "name", "value": "Superseded by ord-1",
          "edited_at": after_now(60), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    let results = body["results"].as_array().unwrap();

    let seen: Vec<(&str, &str)> = results
        .iter()
        .map(|r| (r["client_op_id"].as_str().unwrap(), r["outcome"].as_str().unwrap()))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("ord-1", "accepted"),
            ("ord-2", "rejected"),
            ("ord-3", "accepted"),
            ("ord-4", "rejected"),
            ("ord-5", "superseded"),
        ],
        "results[i] must describe ops[i]"
    );
}

#[tokio::test]
async fn two_users_can_use_the_same_client_op_id() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let mine = app.create_object(&app.client, "Golf", Some("km")).await;
    let my_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(mine["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let other = app.create_user_client("mallory", "another password").await;
    let theirs = app.create_object(&other, "Bike", None).await;
    let their_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(theirs["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    // Op ids are minted by clients, so nothing stops two accounts picking the same one. Each
    // push touches only its own object, so both writes must land: if idempotency were judged
    // on `client_op_id` alone, the second account would be told `accepted` and its write
    // silently dropped -- a lost write reported as success.
    for (client, uuid, name) in [
        (&app.client, &my_uuid, "Golf VII"),
        (&other, &their_uuid, "Brompton"),
    ] {
        let res = client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": "op-shared", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": name,
            "edited_at": after_now(10368060), "device_id": "phone"
        }]))).send().await.unwrap();
        assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    }

    for (uuid, expected) in [(&my_uuid, "Golf VII"), (&their_uuid, "Brompton")] {
        let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
            .bind(uuid).fetch_one(&app.state.db).await.unwrap();
        assert_eq!(&name, expected, "each account's own write must land");
    }

    // Both ops really are in the log, one row per account, not one row shared by the pair.
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-shared'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 2, "the log is keyed by (user_id, client_op_id)");
}

#[tokio::test]
async fn a_foreign_key_field_rejects_a_value_that_is_not_an_id() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(png()).file_name("cover.png").mime_str("image/png").unwrap(),
    );
    let res = app.client.post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(form).send().await.unwrap();
    assert_eq!(res.status(), 201, "upload failed: {}", res.text().await.unwrap());
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-cover", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "cover_attachment_id", "value": attachment_id,
        "edited_at": after_now(13046460), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "accepted");

    // A foreign key holds an id or nothing. SQLite would happily store this string in the
    // integer column, so the ownership check -- which only looks at integers -- must not be
    // the only thing standing between a junk value and the write.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-junk-fk", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "cover_attachment_id", "value": "not-an-id",
        "edited_at": after_now(13132860), "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");

    let cover: Option<i64> = sqlx::query_scalar(
        "SELECT cover_attachment_id FROM objects WHERE client_uuid = $1")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(cover, Some(attachment_id), "the column must be untouched by the rejected op");

    // Null is still how a client clears the reference, and must not be caught by the above.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-clear-fk", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "cover_attachment_id", "value": null,
        "edited_at": after_now(13219260), "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    let cover: Option<i64> = sqlx::query_scalar(
        "SELECT cover_attachment_id FROM objects WHERE client_uuid = $1")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert!(cover.is_none(), "null clears the reference");
}

/// The uuid a client knows a row by. Table names here are literals in this file, never input.
async fn client_uuid(db: &sqlx::AnyPool, table: &str, id: i64) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT client_uuid FROM {table} WHERE id = $1"
    )))
    .bind(id)
    .fetch_one(db)
    .await
    .unwrap()
}

/// Creates an object with one activity, one reminder and one attachment, and returns
/// `(object_id, activity_id, reminder_id, attachment_id)`.
async fn object_with_children(
    app: &common::TestApp,
    client: &reqwest::Client,
    name: &str,
) -> (i64, i64, i64, i64) {
    let object = app.create_object(client, name, Some("km")).await;
    let object_id = object["id"].as_i64().unwrap();

    let res = client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({
            "date": "2026-01-01", "category": "repair", "title": "Timing belt",
            "notes": "", "counter_value": 1000, "cost_cents": 5000
        }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "create activity: {}", res.text().await.unwrap());
    let activity_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    let res = client
        .post(app.url(&format!("/objects/{object_id}/reminders")))
        .json(&json!({ "title": "Service", "due_date": "2026-09-01" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "create reminder: {}", res.text().await.unwrap());
    let reminder_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    let res = client
        .post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(Form::new().part(
            "file",
            Part::bytes(png()).file_name("belt.png").mime_str("image/png").unwrap(),
        ))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "create attachment: {}", res.text().await.unwrap());
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    (object_id, activity_id, reminder_id, attachment_id)
}

/// Every other push test in this file sets a field on an `object`, whose ownership is a column
/// read. The three child entities each reach `objects.user_id` through a join of their own, and
/// those joins are the whole of the authorization check for them.
///
/// Asserting only that a cross-account push comes back `rejected` would pin almost none of
/// that. A join that matched NOTHING would satisfy it while breaking every legitimate push at
/// a child row, so this test has a positive half as well: the owning account's pushes at its
/// own reminder and attachment must be `accepted` and must actually land. And the rejection
/// REASON is asserted, not just the outcome, because "unknown entity_uuid" from an ownership
/// check and the same words from a lookup that can never find anything are the two answers
/// this test exists to tell apart.
///
/// The fixture exists for the third failure mode: a join on the wrong COLUMN. If two accounts
/// each create one object and one child of each kind, every child row's id equals its own
/// `object_id`, so `o.id = r.id` reads exactly like `o.id = r.object_id` and the typo is
/// invisible. Giving the second account rows of its own AND a spare object offsets the two id
/// sequences: each of the victim's child rows then carries an id that names the OTHER account's
/// object, so a wrong-column join both accepts a push it must reject and rejects one it must
/// accept.
#[tokio::test]
async fn one_user_cannot_push_at_another_users_activity_reminder_or_attachment() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // Mallory first, and with one more object than child sets, so no id lines up with itself.
    let mallory = app.create_user_client("mallory", "another password").await;
    app.create_object(&mallory, "Spare", None).await;
    object_with_children(&app, &mallory, "Brompton").await;

    let (object_id, activity_id, reminder_id, attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;
    let reminder_uuid = client_uuid(&app.state.db, "reminders", reminder_id).await;
    let attachment_uuid = client_uuid(&app.state.db, "attachments", attachment_id).await;

    // The premise the fixture buys, stated so it fails loudly rather than quietly rotting if
    // `object_with_children` ever changes what it creates.
    let mallory_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'mallory'")
        .fetch_one(&app.state.db).await.unwrap();
    for (label, child_id) in
        [("activity", activity_id), ("reminder", reminder_id), ("attachment", attachment_id)]
    {
        assert_ne!(child_id, object_id, "the {label} id must not equal its own object_id");
        let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM objects WHERE id = $1")
            .bind(child_id).fetch_optional(&app.state.db).await.unwrap();
        assert_eq!(
            owner,
            Some(mallory_id),
            "the {label} id must name the other account's object, or a join on the wrong \
             column would give the same answer as the right one"
        );
    }

    let res = mallory.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "op-act", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "title", "value": "Stolen activity",
          "edited_at": "2030-01-01T00:00:00Z", "device_id": "mallory-phone" },
        { "client_op_id": "op-rem", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "title", "value": "Stolen reminder",
          "edited_at": "2030-01-01T00:00:00Z", "device_id": "mallory-phone" },
        { "client_op_id": "op-att", "entity": "attachment", "entity_uuid": attachment_uuid,
          "op": "set", "field": "caption", "value": "Stolen caption",
          "edited_at": "2030-01-01T00:00:00Z", "device_id": "mallory-phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    for i in 0..3 {
        assert_eq!(body["results"][i]["outcome"], "rejected", "{body}");
        // The reason an ownership check gives. A lookup that found nothing at all says the
        // same thing, which is exactly why the positive half below has to exist too.
        assert_eq!(body["results"][i]["reason"], "unknown entity_uuid", "{body}");
    }

    let title: String = sqlx::query_scalar("SELECT title FROM activities WHERE id = $1")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(title, "Timing belt", "another account's activity is untouched");
    let title: String = sqlx::query_scalar("SELECT title FROM reminders WHERE id = $1")
        .bind(reminder_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(title, "Service", "another account's reminder is untouched");
    let caption: String = sqlx::query_scalar("SELECT caption FROM attachments WHERE id = $1")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(caption, "", "another account's attachment is untouched");

    // A rejected op is not part of the log, so it must not reach a pull feed either.
    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE client_op_id IN ('op-act', 'op-rem', 'op-att')")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 0, "a rejected op is never logged");

    // The positive half. Each of the same three joins now has to FIND the row: an ownership
    // check that rejects everything is not an ownership check, and the rejections above cannot
    // tell the difference on their own.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "mine-act", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "title", "value": "Timing belt done",
          "edited_at": after_now(2678460), "device_id": "ben-phone" },
        { "client_op_id": "mine-rem", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "title", "value": "Service booked",
          "edited_at": after_now(2678460), "device_id": "ben-phone" },
        { "client_op_id": "mine-att", "entity": "attachment", "entity_uuid": attachment_uuid,
          "op": "set", "field": "caption", "value": "The old belt",
          "edited_at": after_now(2678460), "device_id": "ben-phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    for i in 0..3 {
        assert_eq!(body["results"][i]["outcome"], "accepted", "{body}");
    }

    let title: String = sqlx::query_scalar("SELECT title FROM activities WHERE id = $1")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(title, "Timing belt done", "the owner's own write must land");
    let title: String = sqlx::query_scalar("SELECT title FROM reminders WHERE id = $1")
        .bind(reminder_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(title, "Service booked", "the owner's own write must land");
    let caption: String = sqlx::query_scalar("SELECT caption FROM attachments WHERE id = $1")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(caption, "The old belt", "the owner's own write must land");
}

#[tokio::test]
async fn a_delete_op_tombstones_the_row_and_the_api_stops_serving_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_, activity_id, _, _) = object_with_children(&app, &app.client, "Golf").await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;

    assert_eq!(
        app.client.get(app.url(&format!("/activities/{activity_id}"))).send().await.unwrap().status(),
        200,
        "the row is readable before the delete"
    );

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del", "entity": "activity", "entity_uuid": activity_uuid,
        "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM activities WHERE id = $1")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(rows, 1, "the row survives; only deleted_at is set");
    let deleted: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM activities WHERE id = $1")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert!(deleted.is_some(), "the delete op tombstones the row");

    assert_eq!(
        app.client.get(app.url(&format!("/activities/{activity_id}"))).send().await.unwrap().status(),
        404,
        "a tombstoned activity reads as absent"
    );

    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE client_op_id = 'op-del' AND op = 'delete'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1, "the delete is in the log for other devices to pull");
}

/// A constraint violation used to escape as `sqlx::Error`, which the error mapper turns into a
/// 500. That rolled the whole transaction back, so ops already accepted in the same batch were
/// discarded, and the client's identical retry hit the same op and the same 500 forever: sync
/// stalled permanently on one op the client had no way to identify.
#[tokio::test]
async fn a_constraint_violating_op_is_rejected_without_poisoning_the_batch() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid = client_uuid(&app.state.db, "objects", car["id"].as_i64().unwrap()).await;

    // Ops 2 and 3 violate a NOT NULL and a CHECK constraint respectively; 1 and 4 are ordinary
    // writes that must survive them.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "ok-before", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "description", "value": "Mine",
          "edited_at": after_now(7776060), "device_id": "phone" },
        { "client_op_id": "bad-null", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "name", "value": null,
          "edited_at": after_now(7776060), "device_id": "phone" },
        { "client_op_id": "bad-check", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "counter_unit", "value": "furlongs",
          "edited_at": after_now(7776060), "device_id": "phone" },
        { "client_op_id": "ok-after", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "type", "value": "motorcycle",
          "edited_at": after_now(7776060), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "the batch must not 500: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    let seen: Vec<(&str, &str)> = body["results"].as_array().unwrap().iter()
        .map(|r| (r["client_op_id"].as_str().unwrap(), r["outcome"].as_str().unwrap()))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("ok-before", "accepted"),
            ("bad-null", "rejected"),
            ("bad-check", "rejected"),
            ("ok-after", "accepted"),
        ],
        "one malformed op must not change any other op's outcome: {body}"
    );
    assert!(
        body["results"][1]["reason"].as_str().unwrap().contains("name"),
        "the reason must name the field so the client can drop that op: {body}"
    );
    assert!(
        body["results"][2]["reason"].as_str().unwrap().contains("counter_unit"),
        "the reason must name the field so the client can drop that op: {body}"
    );

    // The transaction stayed usable: the ops either side of the failures really committed, and
    // the failing statements changed nothing.
    let row: (String, String, Option<String>, String) = sqlx::query_as(
        "SELECT name, type, counter_unit, description FROM objects WHERE client_uuid = $1")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(
        row,
        ("Golf".into(), "motorcycle".into(), Some("km".into()), "Mine".into()),
        "accepted ops committed; rejected ops wrote nothing"
    );

    // `op = 'set'` excludes the object's own `create` row (task 9 logs REST creates too, under
    // a freshly minted client_op_id of its own) -- this assertion is about the pushed batch.
    let logged: Vec<String> = sqlx::query_scalar(
        "SELECT client_op_id FROM changes WHERE op = 'set' ORDER BY seq")
        .fetch_all(&app.state.db).await.unwrap();
    assert_eq!(logged, vec!["ok-before", "ok-after"], "only the accepted ops are logged");
}

/// `as_i64()` returns `None` for a float or an out-of-range magnitude, and the old binding fed
/// that `None` straight to SQLite as NULL: the op reported `accepted` and advanced `field_clock`,
/// so the client's correction -- carrying the value's original, earlier `edited_at` -- then lost
/// the last-write-wins comparison and could never repair the row.
#[tokio::test]
async fn a_number_that_is_not_an_integer_is_rejected_and_leaves_the_clock_alone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, activity_id, _, _) = object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "num-float", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "purchase_price_cents", "value": 1250.5,
          "edited_at": after_now(10454460), "device_id": "phone" },
        { "client_op_id": "num-huge", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "counter_value", "value": 100000000000000000000000_i128 as f64,
          "edited_at": after_now(10454460), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    for (i, field) in [(0, "purchase_price_cents"), (1, "counter_value")] {
        assert_eq!(body["results"][i]["outcome"], "rejected", "{body}");
        assert!(
            body["results"][i]["reason"].as_str().unwrap().contains(field),
            "the reason must name the field: {body}"
        );
    }

    // Neither column was written -- the old code stored NULL and called it success.
    let price: Option<i64> = sqlx::query_scalar(
        "SELECT purchase_price_cents FROM objects WHERE client_uuid = $1")
        .bind(&object_uuid).fetch_one(&app.state.db).await.unwrap();
    assert!(price.is_none(), "the row still holds what the REST create put there");
    let counter: Option<i64> = sqlx::query_scalar(
        "SELECT counter_value FROM activities WHERE client_uuid = $1")
        .bind(&activity_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(counter, Some(1000), "the good value the REST create wrote must survive");

    // The repair: the client resends the value it always had, carrying its ORIGINAL edited_at,
    // which is EARLIER than the rejected op's. That only wins if the rejected op left no
    // `field_clock` row behind.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "num-repair", "entity": "object", "entity_uuid": object_uuid,
        "op": "set", "field": "purchase_price_cents", "value": 1250,
        "edited_at": after_now(10368060), "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "the client can still repair: {body}");
    let price: Option<i64> = sqlx::query_scalar(
        "SELECT purchase_price_cents FROM objects WHERE client_uuid = $1")
        .bind(&object_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(price, Some(1250));

    // An integer is still an ordinary accepted value.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "num-int", "entity": "activity", "entity_uuid": activity_uuid,
        "op": "set", "field": "cost_cents", "value": 9900,
        "edited_at": after_now(10540860), "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    let cost: Option<i64> = sqlx::query_scalar(
        "SELECT cost_cents FROM activities WHERE client_uuid = $1")
        .bind(&activity_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(cost, Some(9900));
}

/// A JSON string bound into an INTEGER column used to be `accepted`. SQLite's INTEGER affinity
/// cannot convert `"abc"`, so it stored it verbatim as TEXT, and every read decodes that column
/// as `Option<i64>`: the object's activity list and the activity itself answered 500 from then
/// on. From then on, because the write advanced `field_clock` too, so the client's correction --
/// carrying the value's original, EARLIER `edited_at` -- came back `superseded`. One malformed
/// op from any authenticated client, and the row could never be read or repaired again.
#[tokio::test]
async fn a_value_of_the_wrong_type_for_its_column_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, activity_id, _, _) = object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        // A string into an INTEGER column: the unrepairable 500 above.
        { "client_op_id": "type-str-into-int", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "counter_value", "value": "abc",
          "edited_at": after_now(10454460), "device_id": "phone" },
        // And the milder direction, which corrupts silently: SQLite stores `true` in a TEXT
        // column as '1', so the object's name would have become the string "1".
        { "client_op_id": "type-bool-into-text", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "name", "value": true,
          "edited_at": after_now(10454460), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    for (i, field) in [(0, "counter_value"), (1, "name")] {
        assert_eq!(body["results"][i]["outcome"], "rejected", "{body}");
        assert!(
            body["results"][i]["reason"].as_str().unwrap().contains(field),
            "the reason must name the field: {body}"
        );
    }

    // The columns still hold what they held, in the storage class they are declared with.
    //
    // `typeof` is SQLite's function, and so is the question behind it: only SQLite would have
    // stored the string `"abc"` in an INTEGER column in the first place, so only there is
    // "is this column still holding an integer?" something a test can ask. PostgreSQL cannot
    // put anything but a bigint in a bigint column -- a wrongly typed write is an error, not a
    // silently different storage class -- so the value is the whole of what is left to check.
    let counter: Option<i64> = match app.state.backend {
        logb::dialect::Backend::Sqlite => {
            let stored: (String, Option<i64>) = sqlx::query_as(
                "SELECT typeof(counter_value), counter_value FROM activities WHERE id = $1")
                .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
            assert_eq!(stored.0, "integer", "the integer column is still an integer");
            stored.1
        },
        logb::dialect::Backend::Postgres => sqlx::query_scalar(
            "SELECT counter_value FROM activities WHERE id = $1")
            .bind(activity_id).fetch_one(&app.state.db).await.unwrap(),
    };
    assert_eq!(counter, Some(1000), "the integer column still holds the value it held");
    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE id = $1")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Golf", "the text column is untouched");

    // The reads that used to 500 on a corrupted row.
    for path in [format!("/objects/{object_id}/activities"), format!("/activities/{activity_id}")] {
        assert_eq!(
            app.client.get(app.url(&path)).send().await.unwrap().status(),
            200,
            "{path} must still decode"
        );
    }

    // And the clock did not move, so an edit carrying an EARLIER timestamp -- which is all a
    // correcting client has -- still wins.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "type-repair", "entity": "activity", "entity_uuid": activity_uuid,
        "op": "set", "field": "counter_value", "value": 2000,
        "edited_at": after_now(10368060), "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "the rejected op left no clock: {body}");
    let counter: Option<i64> = sqlx::query_scalar("SELECT counter_value FROM activities WHERE id = $1")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(counter, Some(2000));
}

#[tokio::test]
async fn pull_returns_ops_after_the_cursor_and_advances_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    for (n, name) in [("op-a", "First"), ("op-b", "Second")] {
        app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": n, "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": name,
            // op-a strictly earlier than op-b, one day apart -- the gap is what pull's
            // ordering is pinned against, not the absolute date.
            "edited_at": after_now(5097660 + if n == "op-a" { 0 } else { 86400 }),
            "device_id": "phone"
        }]))).send().await.unwrap();
    }

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    // The object's own `create` is now logged too (task 9), ahead of the two `set` ops.
    assert_eq!(body["changes"].as_array().unwrap().len(), 3);
    assert_eq!(body["complete"], true);
    assert!(body["server_time"].is_string());
    let next = body["next_seq"].as_i64().unwrap();
    let epoch = body["epoch"].as_str().unwrap();

    let body: serde_json::Value = app.client.get(app.url(&format!("/sync/pull?since={next}&epoch={epoch}")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 0, "the cursor is exhausted");
}

#[tokio::test]
async fn pull_pages_and_reports_incompleteness() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    for i in 0..3 {
        app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": format!("op-{i}"), "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "description", "value": format!("note {i}"),
            // Each op a day after the last -- the gaps are what paging is pinned against.
            "edited_at": after_now(7776060 + i as i64 * 86400), "device_id": "phone"
        }]))).send().await.unwrap();
    }

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0&limit=2"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 2);
    assert_eq!(body["complete"], false, "more remains behind the page");
}

/// Every other pull test checks counts; none pins a single field of a `ChangeRow`, so the
/// whole wire contract a phone client is about to be built against is unpinned. `value` is the
/// sharpest trap: `changes.value` stores `op.value.to_string()` (`api::sync::push`), the JSON
/// TEXT of the op's own value, so a plain string arrives double-encoded -- a JSON string
/// literal sitting inside the outer JSON string.
#[tokio::test]
async fn a_pulled_change_rows_fields_match_the_op_that_produced_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf VI", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let edited_at = after_now(60);
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-wire", "entity": "object", "entity_uuid": &uuid,
        "op": "set", "field": "name", "value": "Golf VII",
        "edited_at": &edited_at, "device_id": "phone-42"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "accepted");

    let body: serde_json::Value =
        app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap().json().await.unwrap();
    let row = body["changes"].as_array().unwrap().iter()
        .find(|c| c["op"] == "set" && c["field"] == "name")
        .unwrap_or_else(|| panic!("no set/name row in {body}"));

    assert_eq!(row["entity"], "object");
    assert_eq!(row["entity_uuid"], uuid);
    assert_eq!(row["op"], "set");
    assert_eq!(row["field"], "name");
    assert_eq!(row["value"], "\"Golf VII\"", "a string value arrives double-encoded");
    // `edited_at` was already millisecond-precision (`after_now` uses the same
    // `SecondsFormat::Millis` canonical form), so this also pins canonicalization as
    // idempotent on an already-canonical value, not merely present.
    assert_eq!(row["edited_at"], edited_at);
    assert_eq!(row["device_id"], "phone-42");
    assert!(row["seq"].as_i64().unwrap() > 0);
}

/// Chains partial pages end to end and confirms every row surfaces exactly once, in `seq`
/// order, with `complete` true only on the last one -- the property a phone client actually
/// depends on to catch up without gaps or duplicates.
#[tokio::test]
async fn pull_pages_chain_to_deliver_every_row_exactly_once_in_seq_order() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    // 7 pushed sets, plus the object's own `create` already in the log: 8 rows, which does not
    // divide evenly by the page size below, so the last page is genuinely partial rather than
    // landing on the boundary by luck.
    for i in 0..7 {
        app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": format!("op-{i}"), "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "description", "value": format!("note {i}"),
            "edited_at": after_now(60 + i as i64), "device_id": "phone"
        }]))).send().await.unwrap();
    }

    // Fetched once via bootstrap, off to the side, so it does not add a row to `seen` the way
    // an extra pull would.
    let epoch: serde_json::Value = app.client.get(app.url("/sync/bootstrap"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = epoch["epoch"].as_str().unwrap();

    let mut seen: Vec<i64> = Vec::new();
    let mut since = 0i64;
    loop {
        let body: serde_json::Value = app.client.get(app.url(&format!("/sync/pull?since={since}&limit=3&epoch={epoch}")))
            .send().await.unwrap().json().await.unwrap();
        let changes = body["changes"].as_array().unwrap();
        let complete = body["complete"].as_bool().unwrap();
        assert!(complete || changes.len() == 3, "an incomplete page must be full: {body}");
        for c in changes {
            seen.push(c["seq"].as_i64().unwrap());
        }
        since = body["next_seq"].as_i64().unwrap();
        if complete {
            break;
        }
    }

    assert_eq!(seen.len(), 8, "1 create + 7 sets, across however many pages it took");
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, seen, "every row exactly once, already in seq order");
}

#[tokio::test]
async fn pull_never_leaks_another_users_changes() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-ben", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Ben's",
        "edited_at": after_now(10368060), "device_id": "phone"
    }]))).send().await.unwrap();

    let other = app.create_user_client("mallory", "another password").await;
    let body: serde_json::Value = other.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn a_cursor_before_the_horizon_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-kept", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Kept",
        "edited_at": after_now(13046460), "device_id": "phone"
    }]))).send().await.unwrap();

    // The epoch this database is currently on, fetched before the purge is simulated below --
    // that only rewrites `changes`, not `settings`, so the epoch is unaffected. Carrying it on
    // the request below is what pins this 410 on the horizon rule specifically: without it, an
    // un-epoched non-zero `since` is refused by the epoch check regardless of the horizon, and
    // this test would pass even with the horizon rule deleted entirely.
    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = body["epoch"].as_str().unwrap().to_string();

    // Simulate a purge having removed everything before this row -- including the object's
    // own `create`, which is in the log too now (task 9), or the horizon would still read as
    // the create row's untouched seq and this cursor would look current rather than stale.
    sqlx::query("DELETE FROM changes WHERE client_op_id != 'op-kept'")
        .execute(&app.state.db).await.unwrap();
    sqlx::query("UPDATE changes SET seq = 500 WHERE client_op_id = 'op-kept'")
        .execute(&app.state.db).await.unwrap();

    let res = app.client.get(app.url(&format!("/sync/pull?since=1&epoch={epoch}"))).send().await.unwrap();
    assert_eq!(res.status(), 410, "a stale cursor must be told to re-bootstrap");
}

/// `api::sync::pull` rejects `since < horizon - 1`, not `since <= horizon` or any other
/// off-by-one -- `since == horizon - 1` means the client has already seen the row one below
/// the oldest surviving `seq`, so nothing was purged out from under it; `since == horizon - 2`
/// means it is missing exactly the row the horizon itself sits on. Mutation testing proved the
/// existing tests do not pin the exact boundary: changing `- 1` to `- 2` (the data-losing
/// direction -- a client landing exactly on the boundary would be told 200 and silently
/// resume past a purged row) left `cargo test --test sync` green.
#[tokio::test]
async fn a_cursor_one_below_the_horizon_is_accepted_and_two_below_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-kept", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Kept",
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();

    // The epoch this database is currently on, fetched before the purge is simulated below --
    // that only rewrites `changes`, not `settings`, so the epoch is unaffected.
    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = body["epoch"].as_str().unwrap().to_string();

    // As in the sibling test above: simulate a purge leaving exactly one row, at seq 500, so
    // the horizon (the oldest surviving seq) is 500.
    sqlx::query("DELETE FROM changes WHERE client_op_id != 'op-kept'")
        .execute(&app.state.db).await.unwrap();
    sqlx::query("UPDATE changes SET seq = 500 WHERE client_op_id = 'op-kept'")
        .execute(&app.state.db).await.unwrap();

    let res = app.client.get(app.url(&format!("/sync/pull?since=499&epoch={epoch}"))).send().await.unwrap();
    assert_eq!(res.status(), 200, "since == horizon - 1 (499) has missed nothing and must be accepted");

    // Carrying the epoch here too: without it, this 410 comes from the epoch check (no epoch
    // on a non-zero `since`), not the horizon rule this test exists to pin.
    let res = app.client.get(app.url(&format!("/sync/pull?since=498&epoch={epoch}"))).send().await.unwrap();
    assert_eq!(res.status(), 410, "since == horizon - 2 (498) has missed seq 499 and must be refused");
}

/// `seq` is shared by every account, so a gap wider than one below the horizon does not mean
/// this user lost more than one row -- it can just as easily be a rejected push's burned
/// `client_op_id` claim, or another account's own op, neither of which was ever this user's
/// data to lose. `feed::horizon` cannot tell those apart from a genuinely purged row of this
/// user's own; comparing against `feed::retention_floor` instead does not need to, because
/// nothing above that floor has been purged for anyone.
///
/// Mallory's object anchors the floor low and is never touched. Ben's own log is then trimmed
/// down to a single surviving row far above his cursor, with three rejected ops' burned claims
/// sitting in the gap between them -- exactly the shape the old `since < horizon - 1` rule
/// mistook for missing data, because it judged staleness against Ben's own horizon rather than
/// what the server still retains for anyone. Before the fix this answers 410; the fix must
/// answer 200 and actually resume from where Ben left off.
#[tokio::test]
async fn a_gap_from_a_rejected_push_below_the_boundary_does_not_force_a_rebootstrap() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // Anchors the retention floor at a low `seq` that is never removed, so it stays well below
    // anything Ben's own cursor could be judged against once his own earlier rows are gone.
    let mallory = app.create_user_client("mallory", "another password").await;
    app.create_object(&mallory, "Mallory's ride", None).await;

    let car = app.create_object(&app.client, "Golf", Some("km")).await;

    // The epoch, fetched before any of the manipulation below -- it only touches `changes`, not
    // `settings`, so it stays constant, but a non-zero `since` on a real pull needs one either
    // way.
    let epoch: String = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json::<serde_json::Value>().await.unwrap()["epoch"]
        .as_str().unwrap().to_string();

    // The row Ben's client already has -- this becomes its cursor.
    let kept = app.one_set_op(&car, "Kept", "op-kept").await;
    assert_eq!(app.push_raw(&kept).await.status(), 200);
    let since: i64 = sqlx::query_scalar("SELECT seq FROM changes WHERE client_op_id = 'op-kept'")
        .fetch_one(&app.state.db).await.unwrap();

    // Three ops that never become rows: an unknown `entity_uuid` is rejected by
    // `apply::apply_op` before it touches any table, so each burns exactly the `changes` claim
    // its own `client_op_id` insert made and nothing else.
    for i in 0..3 {
        let body = push_body(json!([{
            "client_op_id": format!("op-burn-{i}"), "entity": "object",
            "entity_uuid": "does-not-exist", "op": "set", "field": "name", "value": "x",
            "edited_at": after_now(60), "device_id": "phone"
        }]));
        let res: serde_json::Value = app.push_raw(&body).await.json().await.unwrap();
        assert_eq!(
            res["results"][0]["outcome"], "rejected",
            "the burn setup itself must reject, or nothing here pins a real gap"
        );
    }

    // One more real row, which will end up as Ben's own new horizon.
    let new = app.one_set_op(&car, "New", "op-new").await;
    assert_eq!(app.push_raw(&new).await.status(), 200);

    // Simulate the purge having reclaimed everything of Ben's own older than `op-new` -- his
    // object's own `create` row and `op-kept` included -- while leaving Mallory's row (and
    // everything else) alone.
    let ben_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'ben'")
        .fetch_one(&app.state.db).await.unwrap();
    sqlx::query("DELETE FROM changes WHERE user_id = $1 AND client_op_id != 'op-new'")
        .bind(ben_id)
        .execute(&app.state.db).await.unwrap();

    let new_horizon: i64 = sqlx::query_scalar("SELECT min(seq) FROM changes WHERE user_id = $1")
        .bind(ben_id)
        .fetch_one(&app.state.db).await.unwrap();
    assert!(
        since < new_horizon - 1,
        "the setup must actually widen the gap past what the old rule tolerated, or this test \
         proves nothing: since={since}, horizon={new_horizon}"
    );

    let res = app.client.get(app.url(&format!("/sync/pull?since={since}&epoch={epoch}")))
        .send().await.unwrap();
    assert_eq!(
        res.status(), 200,
        "a burned claim and another account's own row never held data of Ben's to lose -- \
         resuming from his own last-seen row must not force a re-bootstrap"
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        body["changes"].as_array().unwrap().len(), 1,
        "resuming must actually return the one row Ben has not seen yet"
    );
    assert_eq!(body["next_seq"], new_horizon, "the one row returned must be his new horizon");
}

#[tokio::test]
async fn a_cursor_against_an_emptied_log_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // The epoch, so the assertion below is pinned on the horizon rule (empty log, so horizon
    // is 0) rather than on the epoch check, which a bare non-zero `since` would also trip.
    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = body["epoch"].as_str().unwrap().to_string();

    // A non-zero cursor can only have come from ops that existed, so an empty log means they
    // were purged. Answering 200 here would let the client believe it is current forever.
    let res = app.client.get(app.url(&format!("/sync/pull?since=7&epoch={epoch}"))).send().await.unwrap();
    assert_eq!(res.status(), 410);

    // A first pull is still legal against the same empty log.
    let res = app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn bootstrap_returns_live_rows_and_a_resumable_cursor() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let keep = app.create_object(&app.client, "Golf", Some("km")).await;
    let drop = app.create_object(&app.client, "Old Bike", None).await;
    let drop_id = drop["id"].as_i64().unwrap();
    assert_eq!(
        app.client.delete(app.url(&format!("/objects/{drop_id}"))).send().await.unwrap().status(),
        204
    );

    let body: serde_json::Value = app.client.get(app.url("/sync/bootstrap"))
        .send().await.unwrap().json().await.unwrap();

    let objects = body["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 1, "the tombstoned object is absent");
    assert_eq!(objects[0]["name"], keep["name"]);
    assert!(objects[0]["client_uuid"].is_string(), "rows are addressable by uuid");
    assert!(body["seq"].is_i64());
    assert!(body["server_time"].is_string());

    // The cursor is immediately usable, together with the epoch it was issued alongside.
    let seq = body["seq"].as_i64().unwrap();
    let epoch = body["epoch"].as_str().unwrap();
    let res = app.client.get(app.url(&format!("/sync/pull?since={seq}&epoch={epoch}"))).send().await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn bootstrap_is_scoped_to_the_caller() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let other = app.create_user_client("mallory", "another password").await;
    let body: serde_json::Value = other.get(app.url("/sync/bootstrap"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["objects"].as_array().unwrap().len(), 0);
}

/// The sibling above only pins `objects` -- `feed::snapshot`'s activities/reminders/
/// attachments/files queries are four more statements, each with its own `WHERE`, and nothing
/// about the objects check proves any of them are scoped. Mutation testing confirmed it:
/// stripping `o.user_id = ?` and `o.deleted_at IS NULL` from all four left the whole suite
/// green. This pins two failure modes those clauses guard against: another account's rows
/// leaking in (the `user_id`/join half), and a LIVE child surviving in a snapshot because its
/// parent object was tombstoned without it (the `o.deleted_at IS NULL` half) -- which the
/// child's own `deleted_at IS NULL` filter cannot catch on its own, since the child really is
/// live.
#[tokio::test]
async fn bootstrap_scopes_every_child_table_and_hides_a_live_child_of_a_tombstoned_object() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    object_with_children(&app, &app.client, "Golf").await;

    // -- Cross-account: a second account with an object of its own, and none of ben's rows,
    // must see none of ben's activities, reminders, attachments or files.
    let mallory = app.create_user_client("mallory", "another password").await;
    app.create_object(&mallory, "Bike", None).await;

    let body: serde_json::Value = mallory.get(app.url("/sync/bootstrap"))
        .send().await.unwrap().json().await.unwrap();
    for table in ["activities", "reminders", "attachments", "files"] {
        assert_eq!(
            body[table].as_array().unwrap().len(), 0,
            "mallory's bootstrap must not carry ben's {table}: {body}"
        );
    }

    // -- A live child of a tombstoned object. `api::objects::delete` always cascades the
    // tombstone to every child, so reaching "object gone, child still live" needs a raw update
    // that bypasses the cascade -- exactly the shape a bug in that cascade (or a hand-run
    // migration) would leave behind, and precisely what the join's `o.deleted_at IS NULL` has
    // to catch since the child rows here are otherwise ordinary and live.
    let (orphan_object, orphan_activity, orphan_reminder, orphan_attachment) =
        object_with_children(&app, &app.client, "Orphaned").await;
    sqlx::query("UPDATE objects SET deleted_at = $1 WHERE id = $2")
        .bind(logb::db::now()).bind(orphan_object).execute(&app.state.db).await.unwrap();
    let object_deleted: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM objects WHERE id = $1")
        .bind(orphan_object).fetch_one(&app.state.db).await.unwrap();
    assert!(object_deleted.is_some(), "fixture setup: the object must be tombstoned");
    for (table, id) in [
        ("activities", orphan_activity), ("reminders", orphan_reminder), ("attachments", orphan_attachment),
    ] {
        let deleted: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT deleted_at FROM {table} WHERE id = $1"
        )))
        .bind(id).fetch_one(&app.state.db).await.unwrap();
        assert!(deleted.is_none(), "fixture setup: the {table} row must still be live");
    }

    let body: serde_json::Value = app.client.get(app.url("/sync/bootstrap"))
        .send().await.unwrap().json().await.unwrap();
    for table in ["activities", "reminders", "attachments"] {
        assert!(
            body[table].as_array().unwrap().iter().all(|r| r["object_id"] != orphan_object),
            "a live child of a tombstoned object must not appear in {table}: {body}"
        );
    }
}

#[tokio::test]
async fn purge_drops_old_log_rows_and_old_tombstones() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();

    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-ancient", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Ancient",
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();

    // Backdate both the log row and a tombstone well past any sane window.
    sqlx::query("UPDATE changes SET applied_at = '2000-01-01T00:00:00Z'")
        .execute(&app.state.db).await.unwrap();
    sqlx::query("UPDATE objects SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = $1")
        .bind(object_id).execute(&app.state.db).await.unwrap();

    let removed = logb::sync::feed::purge(&app.state, 90).await.unwrap();
    // The blanket backdate above ages out every `changes` row for this user, including the
    // object's own `create` (task 9 logs REST creates too), not only the pushed `set`.
    assert_eq!(removed, 2, "the ancient log rows went");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(rows, 0);

    let objects: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(objects, 0, "an expired tombstone is finally a real delete");
}

/// The purge must not hard-delete a tombstoned parent while any object -- live, or itself
/// tombstoned but not yet purged -- still names it as `parent_id`. `objects.parent_id`
/// references `objects(id)` with no `ON DELETE` action, so taking the parent first, in any purge
/// run where the child's own row survives that same statement -- its tombstone still fresh, or
/// the child itself held back by one of the other three guards -- fails the entire purge on the
/// foreign key. An ordinary cascade delete tombstones a whole subtree under one shared
/// timestamp, so parent and child usually age out and get purged together in the same statement,
/// where no violation occurs; this guard exists for the case where they don't, and that is the
/// case this test stages by backdating the parent's tombstone alone. Were the reference ever
/// relaxed it would instead leave the child pointing at a row that no longer exists. Either way
/// the parent waits, exactly as it already waits for its activities, reminders and attachments.
#[tokio::test]
async fn a_tombstoned_parent_is_not_purged_while_a_tombstoned_child_still_references_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    let garage_id = garage["id"].as_i64().unwrap();
    let light_id = light["id"].as_i64().unwrap();

    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2")
        .bind(garage_id).bind(light_id)
        .execute(&app.state.db).await.unwrap();

    // The REST delete tombstones the garage and cascades a tombstone onto the light.
    app.delete_object(&garage).await;
    // Backdate the parent's tombstone alone -- the same date `age_out_tombstones` uses, applied
    // to one row -- so the parent is eligible for this run and the child, whose tombstone is
    // minutes old, is not. That is the only arrangement that puts a real foreign key check
    // between two rows: with both eligible they would go in one statement, where a no-action
    // constraint is checked at the end and sees nothing wrong.
    sqlx::query("UPDATE objects SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = $1")
        .bind(garage_id)
        .execute(&app.state.db).await.unwrap();

    app.run_purge().await;

    let parent: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(garage_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(parent, 1, "the parent must survive while a child still names it");
    let child: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(light_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(child, 1, "the child's tombstone is still inside the window and stays");

    // Once the child has aged out too it goes, and the guard -- which reads the table as the
    // statement found it -- still holds the parent back for that run, so the parent leaves on
    // the next one. That is the guard's whole cost: one extra run per level of nesting.
    app.age_out_tombstones().await;
    app.run_purge().await;
    let child: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(light_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(child, 0, "the aged-out child goes on this run");

    app.run_purge().await;
    let parent: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(garage_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(parent, 0, "once nothing names it the parent is finally purged");
}

/// The purge's one silent forever-retention, said out loud.
///
/// The `objects` guard asks whether any row still names this one as its parent, and carries no
/// `c.id <> objects.id`, so a row that is its own parent answers its own guard on every run and
/// is never purged. No validated write path can produce one -- both doors go through
/// `record::parent_is_valid`, and `objects::update` asks it under the write lock -- so this is
/// an import or a hand edit, and the row is planted here the same way. Being held back is the
/// safe outcome and is not what this test changes; what it pins is that an operator can *see*
/// it, rather than a tombstone quietly outliving its retention window with no error and no log
/// line. Delete the `warn!` in `sync::feed::purge` and this fails while the retention assertion
/// above it still passes.
#[tokio::test]
async fn a_self_parenting_tombstone_is_held_back_and_says_so() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let orphan = app.create_object(&app.client, "Ouroboros", None).await;
    let id = orphan["id"].as_i64().unwrap();
    app.delete_object(&orphan).await;
    // Past the validation, exactly as an import or a hand-edited database could.
    sqlx::query("UPDATE objects SET parent_id = id WHERE id = $1")
        .bind(id)
        .execute(&app.state.db).await.unwrap();
    app.age_out_tombstones().await;

    app.run_purge().await;

    let still_there: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(still_there, 1, "the guard holds a self-parenting row back, which is the safe half");
    let logs = app.captured_logs();
    assert!(
        logs.contains("name themselves as their own parent"),
        "the purge must warn about a row it can never remove, not drop it silently",
    );
}

#[tokio::test]
async fn purge_keeps_recent_history() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-fresh", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Fresh",
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();

    assert_eq!(logb::sync::feed::purge(&app.state, 90).await.unwrap(), 0);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db).await.unwrap();
    // The object's own `create` is logged too now, alongside the pushed `set`.
    assert_eq!(rows, 2, "today's history is not history yet");
}

#[tokio::test]
async fn purge_reclaims_the_blob_of_an_expired_attachment() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"%PDF-1.4 fake".to_vec())
            .file_name("invoice.pdf")
            .mime_str("application/pdf")
            .unwrap(),
    );
    let res = app.client.post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(form).send().await.unwrap();
    assert_eq!(res.status(), 201, "upload failed: {}", res.text().await.unwrap());
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    let sha: String = sqlx::query_scalar(
        "SELECT f.sha256 FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = $1")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    let blob = app.state.storage.blob_path(&sha);
    assert!(blob.exists(), "the upload landed on disk");

    assert_eq!(
        app.client.delete(app.url(&format!("/attachments/{attachment_id}")))
            .send().await.unwrap().status(),
        204
    );
    assert!(blob.exists(), "a tombstoned attachment still pins its blob");

    sqlx::query("UPDATE attachments SET deleted_at = '2000-01-01T00:00:00Z'")
        .execute(&app.state.db).await.unwrap();
    logb::sync::feed::purge(&app.state, 90).await.unwrap();

    assert!(!blob.exists(), "an expired tombstone finally frees the bytes");
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM files")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(files, 0, "the files row goes with its last attachment");
}

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

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-object", "entity": "object", "entity_uuid": object_uuid,
        "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    for (table, id) in
        [("activities", activity_id), ("reminders", reminder_id), ("attachments", attachment_id)]
    {
        let deleted: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT deleted_at FROM {table} WHERE id = $1"
        )))
        .bind(id).fetch_one(&app.state.db).await.unwrap();
        assert!(deleted.is_some(), "the {table} row must be tombstoned with its object");
    }

    // A tombstone nobody is told about is the same as no tombstone at all for a device that
    // was offline, so each cascaded child has to reach the feed on its own uuid.
    for (entity, uuid) in [
        ("activity", &activity_uuid),
        ("reminder", &reminder_uuid),
        ("attachment", &attachment_uuid),
    ] {
        let logged: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM changes WHERE entity = $1 AND entity_uuid = $2 AND op = 'delete'")
            .bind(entity).bind(uuid).fetch_one(&app.state.db).await.unwrap();
        assert_eq!(logged, 1, "the cascaded {entity} delete must be in the log for other devices");
    }

    let res = app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    let uuids: Vec<&str> = body["changes"].as_array().unwrap().iter()
        .filter(|c| c["op"] == "delete")
        .map(|c| c["entity_uuid"].as_str().unwrap())
        .collect();
    for uuid in [&object_uuid, &activity_uuid, &reminder_uuid, &attachment_uuid] {
        assert!(uuids.contains(&uuid.as_str()), "pull must carry the delete of {uuid}: {uuids:?}");
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

    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({
            "date": "2026-01-01", "category": "repair", "title": "Timing belt",
            "notes": "", "counter_value": 1000, "cost_cents": 5000
        }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "create activity: {}", res.text().await.unwrap());
    let activity_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(
            Form::new()
                .text("activity_id", activity_id.to_string())
                .part("file", Part::bytes(png()).file_name("belt.png").mime_str("image/png").unwrap()),
        )
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "create attachment: {}", res.text().await.unwrap());
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;
    let attachment_uuid = client_uuid(&app.state.db, "attachments", attachment_id).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-activity", "entity": "activity", "entity_uuid": activity_uuid,
        "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    let deleted: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM attachments WHERE id = $1")
            .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    assert!(deleted.is_some(), "the activity's attachment must be tombstoned with it");

    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE entity = 'attachment' AND entity_uuid = $1 \
         AND op = 'delete'")
        .bind(&attachment_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1, "the cascaded attachment delete must be in the log");
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
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-set-cover", "entity": "object", "entity_uuid": object_uuid,
        "op": "set", "field": "cover_attachment_id", "value": attachment_id,
        "edited_at": after_now(7776060), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "set cover failed: {}", res.text().await.unwrap());
    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE id = $1")
            .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(cover, Some(attachment_id), "fixture setup: the cover must be set before deletion");

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-attachment", "entity": "attachment", "entity_uuid": attachment_uuid,
        "op": "delete", "edited_at": after_now(7862460), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE id = $1")
            .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert!(cover.is_none(), "a pushed attachment delete must clear the object's cover, same as REST");
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
        .bind(stale).bind(object_id).execute(&app.state.db).await.unwrap();
    sqlx::query("UPDATE activities SET updated_at = $1 WHERE id = $2")
        .bind(stale).bind(activity_id).execute(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-object", "entity": "object", "entity_uuid": object_uuid,
        "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    let object_updated: String = sqlx::query_scalar("SELECT updated_at FROM objects WHERE id = $1")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_ne!(object_updated, stale, "the object's own tombstone must bump updated_at");

    let activity_updated: String =
        sqlx::query_scalar("SELECT updated_at FROM activities WHERE id = $1")
            .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_ne!(activity_updated, stale, "a cascaded activity tombstone must bump updated_at too");
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
        .bind(stale).bind(activity_id).execute(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-activity", "entity": "activity", "entity_uuid": activity_uuid,
        "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    let activity_updated: String =
        sqlx::query_scalar("SELECT updated_at FROM activities WHERE id = $1")
            .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_ne!(activity_updated, stale, "a directly-deleted activity must bump its own updated_at");
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
        "SELECT f.sha256 FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = $1")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    let blob = app.state.storage.blob_path(&sha);
    assert!(blob.exists(), "the fixture's upload landed on disk");

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-object", "entity": "object", "entity_uuid": object_uuid,
        "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    // Only the object's tombstone is aged out. Its children were tombstoned just now, so they
    // are still inside the window and the purge has no business removing them yet -- through
    // the cascade or otherwise.
    sqlx::query("UPDATE objects SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = $1")
        .bind(object_id).execute(&app.state.db).await.unwrap();
    logb::sync::feed::purge(&app.state, 90).await.unwrap();

    for (table, id) in
        [("activities", activity_id), ("reminders", reminder_id), ("attachments", attachment_id)]
    {
        let rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE id = $1"
        )))
        .bind(id).fetch_one(&app.state.db).await.unwrap();
        assert_eq!(rows, 1, "the {table} row is still inside the window and must survive");
    }
    assert!(blob.exists(), "no attachment has aged out, so its blob must still be pinned");
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM files")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(files, 1, "the files row must not be orphaned by a destroyed attachment");

    // And the wait is only a wait: once the children's own tombstones age out too, the purge
    // finishes the job and reclaims the bytes. Holding a parent back must not wedge it.
    for table in ["activities", "reminders", "attachments"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET deleted_at = '2000-01-01T00:00:00Z'"
        )))
        .execute(&app.state.db).await.unwrap();
    }
    logb::sync::feed::purge(&app.state, 90).await.unwrap();

    for table in ["objects", "activities", "reminders", "attachments", "files"] {
        let rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table}"
        )))
        .fetch_one(&app.state.db).await.unwrap();
        assert_eq!(rows, 0, "{table} should be empty once every tombstone has aged out");
    }
    assert!(!blob.exists(), "the expired attachment finally frees the bytes");
}

/// `client_uuid` is nullable on all five tables, so a database can hold a row without one.
/// `entity_uuid NOT IN (SELECT client_uuid ...)` is UNKNOWN for every row the moment that
/// subquery yields one NULL, which turned the orphan sweep into a permanent no-op for the whole
/// database. The sweep's fix (`NOT EXISTS`, one correlated clause per table) is structurally
/// identical across `objects`, `activities`, `reminders`, `attachments` and `files`, so a test
/// that plants the NULL in only one of them proves nothing about the other four -- a clause
/// that regressed back to the `NOT IN` shape on any one of them would pass a single-table test
/// unnoticed. This drives the same scenario once per table, planting the NULL row in a
/// different table each time.
#[tokio::test]
async fn an_orphaned_field_clock_row_is_swept_despite_a_null_client_uuid_in_any_table() {
    for legacy_table in ["objects", "activities", "reminders", "attachments", "files"] {
        let app = common::spawn().await;
        app.setup("ben", "correct horse").await;
        let car = app.create_object(&app.client, "Golf", Some("km")).await;
        let object_id = car["id"].as_i64().unwrap();
        let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;
        let user_id: i64 = sqlx::query_scalar("SELECT user_id FROM objects WHERE id = $1")
            .bind(object_id).fetch_one(&app.state.db).await.unwrap();

        // A live clock the sweep must leave alone, so a run that swept everything -- rather
        // than only the orphan -- would still be caught.
        let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": "op-name", "entity": "object", "entity_uuid": &object_uuid,
            "op": "set", "field": "name", "value": "Renamed",
            "edited_at": after_now(7776060), "device_id": "phone"
        }]))).send().await.unwrap();
        assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

        // A row from before `client_uuid` existed, or from any writer that never set it, in the
        // table under test this iteration -- every column each table's NOT NULL constraints
        // require, and nothing that names `client_uuid`, so it defaults NULL.
        match legacy_table {
            "objects" => {
                sqlx::query(
                    "INSERT INTO objects (user_id, name, type, created_at, updated_at) \
                     VALUES ($1, 'Legacy', 'car', '2026-03-05T00:00:00Z', '2026-03-05T00:00:00Z')")
                    .bind(user_id).execute(&app.state.db).await.unwrap();
            }
            "activities" => {
                sqlx::query(
                    "INSERT INTO activities \
                     (object_id, date, category, title, notes, created_at, updated_at) \
                     VALUES ($1, '2026-03-05', 'other', 'Legacy', '', \
                             '2026-03-05T00:00:00Z', '2026-03-05T00:00:00Z')")
                    .bind(object_id).execute(&app.state.db).await.unwrap();
            }
            "reminders" => {
                sqlx::query(
                    "INSERT INTO reminders (object_id, title, due_date, created_at) \
                     VALUES ($1, 'Legacy', '2026-09-01', '2026-03-05T00:00:00Z')")
                    .bind(object_id).execute(&app.state.db).await.unwrap();
            }
            "attachments" => {
                let file_id: i64 = sqlx::query_scalar(
                    "INSERT INTO files (user_id, sha256, original_name, mime, size, created_at) \
                     VALUES ($1, 'deadbeef', 'legacy.png', 'image/png', 1, '2026-03-05T00:00:00Z') \
                     RETURNING id")
                    .bind(user_id).fetch_one(&app.state.db).await.unwrap();
                sqlx::query(
                    "INSERT INTO attachments (object_id, file_id, kind, created_at) \
                     VALUES ($1, $2, 'photo', '2026-03-05T00:00:00Z')")
                    .bind(object_id).bind(file_id).execute(&app.state.db).await.unwrap();
            }
            "files" => {
                sqlx::query(
                    "INSERT INTO files (user_id, sha256, original_name, mime, size, created_at) \
                     VALUES ($1, 'deadbeef', 'legacy.png', 'image/png', 1, '2026-03-05T00:00:00Z')")
                    .bind(user_id).execute(&app.state.db).await.unwrap();
            }
            other => unreachable!("not one of the five tables: {other}"),
        }

        // A clock for a uuid no table carries any more: the sweep's whole reason to exist.
        sqlx::query(
            "INSERT INTO field_clock (entity, entity_uuid, field, edited_at, device_id) \
             VALUES ('activity', 'gone-with-the-row', 'title', '2026-01-01T00:00:00Z', 'phone')")
            .execute(&app.state.db).await.unwrap();

        logb::sync::feed::purge(&app.state, 90).await.unwrap();

        let orphans: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM field_clock WHERE entity_uuid = 'gone-with-the-row'")
            .fetch_one(&app.state.db).await.unwrap();
        assert_eq!(
            orphans, 0,
            "the orphaned clock must be swept even beside a NULL client_uuid in {legacy_table}"
        );

        let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM field_clock WHERE entity_uuid = $1")
            .bind(&object_uuid).fetch_one(&app.state.db).await.unwrap();
        // 11, not 1: the object's own REST `create` stamps every field in `Entity::Object`'s
        // whitelist (task 9), and the pushed `set` above only overwrites `name`'s entry rather
        // than adding a twelfth. All 11 must survive the sweep untouched. It was 9 until
        // `parent_id` joined the whitelist and 10 until `tags` did -- this count is deliberately
        // a literal so that widening the whitelist has to be noticed here.
        assert_eq!(
            kept, 11,
            "a clock for a row that still exists must be left alone (NULL planted in {legacy_table})"
        );
    }
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
        "SELECT f.client_uuid FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = $1")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-file", "entity": "file", "entity_uuid": file_uuid,
        "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");

    let deleted: Option<String> = sqlx::query_scalar(
        "SELECT deleted_at FROM files WHERE client_uuid = $1")
        .bind(&file_uuid).fetch_one(&app.state.db).await.unwrap();
    assert!(deleted.is_none(), "a file must never be tombstoned over sync");

    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE client_op_id = 'op-del-file'")
        .fetch_one(&app.state.db).await.unwrap();
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
    let ids: Vec<String> = cases.as_array().unwrap().iter()
        .map(|c| c["client_op_id"].as_str().unwrap().to_string()).collect();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(cases)).send().await.unwrap();
    assert_eq!(res.status(), 200, "the batch must not 500: {}", res.text().await.unwrap());
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
    assert_eq!((name.as_str(), obj_type.as_str(), purchase_date, price), ("Golf", "car", None, None));

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
        (rtitle.as_str(), rdue_date.as_deref(), rdue_counter, rrepeat_months, rrepeat_counter),
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

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "junk-create", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "create", "field": "title", "value": "should not be stored",
          "edited_at": after_now(13132860), "device_id": "phone" },
        { "client_op_id": "junk-delete", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "delete", "field": "title", "value": "should not be stored either",
          "edited_at": after_now(13132861), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    assert_eq!(body["results"][1]["outcome"], "accepted", "{body}");

    let rows: Vec<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT field, value FROM changes WHERE client_op_id IN ('junk-create', 'junk-delete') ORDER BY seq")
        .fetch_all(&app.state.db).await.unwrap();
    for (field, value) in rows {
        assert_eq!((field, value), (None, None), "create and delete never carry a field or value");
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

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-set-cover", "entity": "object", "entity_uuid": object_uuid,
        "op": "set", "field": "cover_attachment_id", "value": attachment_id,
        "edited_at": after_now(13046460), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "set cover failed: {}", res.text().await.unwrap());

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-kind-away", "entity": "attachment", "entity_uuid": attachment_uuid,
        "op": "set", "field": "kind", "value": "document",
        "edited_at": after_now(13132860), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");

    let kind: String = sqlx::query_scalar("SELECT kind FROM attachments WHERE id = $1")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(kind, "photo", "the cover's kind must not change while it is still the cover");
    let cover: Option<i64> = sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE id = $1")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(cover, Some(attachment_id), "the cover pointer must be unaffected");

    // Once it is no longer the cover, the same edit is an ordinary accepted write.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-clear-cover", "entity": "object", "entity_uuid": object_uuid,
        "op": "set", "field": "cover_attachment_id", "value": null,
        "edited_at": after_now(13219260), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-kind-ok", "entity": "attachment", "entity_uuid": attachment_uuid,
        "op": "set", "field": "kind", "value": "document",
        "edited_at": after_now(13305660), "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    let kind: String = sqlx::query_scalar("SELECT kind FROM attachments WHERE id = $1")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(kind, "document", "with no cover pinning it, kind is free to change");
}

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
        .bind(id).fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "phone-op-1", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Phone Golf",
        "edited_at": after_now(31536060), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "accepted");

    // The browser edits the same field over REST. Every other field is resent unchanged so
    // only `name` moves. This unconditionally overwrites `field_clock` to the real current
    // instant, regardless of the phone's on-paper-later stamp -- a REST write always reflects
    // what just happened.
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Browser Golf", "type": "car", "counter_unit": "km", "description": "",
        "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "phone-op-2", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Late Phone Golf",
        "edited_at": before_now(60), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "superseded", "{body}");

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE id = $1")
        .bind(id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Browser Golf", "the REST edit must survive a sync op stamped before it");
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
        .bind(id).fetch_one(&app.state.db).await.unwrap();

    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf VII", "type": "car", "counter_unit": "km", "description": "",
        "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    assert_eq!(
        app.client.delete(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(),
        204
    );

    let res = app.client.get(app.url("/sync/pull")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    let mine: Vec<serde_json::Value> = body["changes"].as_array().unwrap().iter()
        .filter(|c| c["entity_uuid"] == uuid).cloned().collect();
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
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(before, 1, "the create itself is logged");

    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "description": "",
        "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db).await.unwrap();
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

    let res = app.client.delete(app.url(&format!("/objects/{object_id}"))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    for (uuid, label) in [
        (&object_uuid, "object"), (&activity_uuid, "activity"),
        (&reminder_uuid, "reminder"), (&attachment_uuid, "attachment"),
    ] {
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT op, field FROM changes WHERE entity_uuid = $1 AND op = 'delete'")
            .bind(uuid).fetch_optional(&app.state.db).await.unwrap();
        let (op, field) = row.unwrap_or_else(|| panic!("no delete logged for {label} ({uuid})"));
        assert_eq!(op, "delete", "{label}");
        assert!(field.is_none(), "{label}: create and delete never carry a field");
    }
}

/// Settings and user rows are not part of the sync protocol -- `sync::whitelist` only ever
/// names object/activity/reminder/attachment/file -- so writing them must never touch
/// `changes`, however the write happened.
#[tokio::test]
async fn settings_and_user_writes_produce_no_changes_rows() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let res = app.client.put(app.url("/settings")).json(&json!({ "currency": "USD" })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let _ = app.create_user_client("mallory", "correct horse").await;

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(count, 0, "settings and user writes are outside the sync protocol");
}

#[tokio::test]
async fn a_pull_carrying_a_stale_epoch_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let _ = car;

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = body["epoch"].as_str().expect("pull states the epoch").to_string();
    let next = body["next_seq"].as_i64().unwrap();
    assert!(next > 0, "the REST create is already in the feed");

    // The cursor is current and the epoch matches: ordinary catch-up.
    let res = app.client.get(app.url(&format!("/sync/pull?since={next}&epoch={epoch}")))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);

    // Same cursor, an epoch from a different database: the numbers no longer mean what the
    // device thinks they mean.
    let res = app.client.get(app.url(&format!("/sync/pull?since={next}&epoch=not-this-database")))
        .send().await.unwrap();
    assert_eq!(res.status(), 410);

    // A non-zero cursor with no epoch at all is the same failure: a client that cannot say
    // which database it is resuming against cannot safely resume.
    let res = app.client.get(app.url(&format!("/sync/pull?since={next}")))
        .send().await.unwrap();
    assert_eq!(res.status(), 410);

    // A first pull carries no cursor, so it needs no epoch.
    assert_eq!(app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap().status(), 200);

    // The rule is `since > 0 && epoch mismatch`, on a single `&&` -- pin that a garbage epoch
    // alongside `since=0` still passes, so a future edit cannot accidentally start checking the
    // epoch on a first pull too.
    let res = app.client.get(app.url("/sync/pull?since=0&epoch=garbage")).send().await.unwrap();
    assert_eq!(res.status(), 200, "since=0 needs no epoch, garbage or otherwise");
}

#[tokio::test]
async fn bootstrap_states_the_epoch_it_belongs_to() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let body: serde_json::Value = app.client.get(app.url("/sync/bootstrap"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = body["epoch"].as_str().expect("bootstrap states the epoch");

    // The pair is usable together: the seq and epoch a bootstrap hands out are accepted by pull.
    let seq = body["seq"].as_i64().unwrap();
    let res = app.client.get(app.url(&format!("/sync/pull?since={seq}&epoch={epoch}")))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn rotating_the_epoch_forces_every_device_to_re_bootstrap() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = body["epoch"].as_str().unwrap().to_string();
    let next = body["next_seq"].as_i64().unwrap();

    let fresh = logb::sync::epoch::rotate(&app.state.db).await.unwrap();
    assert_ne!(fresh, epoch, "rotation produces a different epoch");

    let res = app.client.get(app.url(&format!("/sync/pull?since={next}&epoch={epoch}")))
        .send().await.unwrap();
    assert_eq!(res.status(), 410, "the device's epoch is now the old database's");
}

/// `rotate` must not report success when nothing actually changed. A plain `UPDATE ... WHERE
/// key = 'sync_epoch'` matches zero rows if that row is ever absent, and would still return a
/// freshly minted uuid to the caller -- `--restore` would print "sync epoch is now ..." and
/// exit 0 while the database goes on advertising its old identity (or, here, none at all, which
/// then makes every pull 500 through `current`'s `fetch_one`).
#[tokio::test]
async fn rotate_heals_a_missing_row_instead_of_silently_reporting_a_fake_success() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    sqlx::query("DELETE FROM settings WHERE key = 'sync_epoch'")
        .execute(&app.state.db).await.unwrap();

    let fresh = logb::sync::epoch::rotate(&app.state.db).await
        .expect("rotate must not silently no-op when the row is missing");

    let value: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'sync_epoch'")
        .fetch_one(&app.state.db).await
        .expect("rotate reported success, but the row it claims to have set is not there");
    assert_eq!(value, fresh, "the row actually on disk must match what rotate reported");
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
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "op-nest", "entity": "object", "entity_uuid": garage_uuid,
          "op": "set", "field": "parent_id", "value": house["id"].as_i64().unwrap(),
          "edited_at": after_now(60), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "a legitimate parent must land: {body}");

    // Now a device tries to push the reverse: House becomes Garage's child.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "op-cycle", "entity": "object", "entity_uuid": house_uuid,
          "op": "set", "field": "parent_id", "value": garage["id"].as_i64().unwrap(),
          "edited_at": after_now(60), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "the batch must not 500");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "a cycle must be rejected, not applied");

    let parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM objects WHERE client_uuid = $1")
        .bind(&house_uuid).fetch_one(&app.state.db).await.unwrap();
    assert!(parent.is_none(), "the cycle must not have landed");
}

#[tokio::test]
async fn every_pulled_change_names_the_servers_id_for_its_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let activity: serde_json::Value = app.client
        .post(app.url(&format!("/objects/{}/activities", car["id"])))
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "Wipers" }))
        .send().await.unwrap().json().await.unwrap();
    // A create logs only a `create` row; an edit is what produces a `set` row to check.
    let res = app.client.patch(app.url(&format!("/activities/{}", activity["id"])))
        .json(&json!({ "date": "2026-09-01", "category": "repair", "title": "Wiper blades" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);

    let pulled: serde_json::Value = app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap().json().await.unwrap();
    let changes = pulled["changes"].as_array().unwrap();
    let object_create = changes.iter().find(|c| c["entity"] == "object" && c["op"] == "create").unwrap();
    assert_eq!(object_create["entity_id"], car["id"]);
    let activity_create = changes.iter().find(|c| c["entity"] == "activity" && c["op"] == "create").unwrap();
    assert_eq!(activity_create["entity_id"], activity["id"]);
    // A set row names the same id as the create for the same uuid.
    let activity_set = changes.iter().find(|c| c["entity"] == "activity" && c["op"] == "set").unwrap();
    assert_eq!(activity_set["entity_id"], activity["id"]);
}

#[tokio::test]
async fn deleting_a_cover_attachment_logs_the_cover_being_cleared() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let a: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(Form::new().part("file", Part::bytes(png()).file_name("a.png").mime_str("image/png").unwrap()))
        .send().await.unwrap().json().await.unwrap();
    // Make it the cover, then delete it.
    let res = app.client.patch(app.url(&format!("/objects/{id}")))
        .json(&json!({ "name": "Golf", "type": "car", "counter_unit": "km", "cover_attachment_id": a["id"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let before: serde_json::Value = app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap().json().await.unwrap();
    let since = before["next_seq"].as_i64().unwrap();
    let epoch = before["epoch"].as_str().unwrap().to_string();
    let res = app.client.delete(app.url(&format!("/attachments/{}", a["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    let after: serde_json::Value = app.client.get(app.url(&format!("/sync/pull?since={since}&epoch={epoch}"))).send().await.unwrap().json().await.unwrap();
    let cleared = after["changes"].as_array().unwrap().iter().find(|c|
        c["entity"] == "object" && c["op"] == "set" && c["field"] == "cover_attachment_id");
    let cleared = cleared.expect("the cover clear is in the feed");
    assert_eq!(cleared["entity_id"], id);
    // A REST-side write stores the double-encoded JSON `null` (the string "null"), while a
    // pushed op with an explicit null stores SQL NULL; a client must read both as "clear".
    assert!(cleared["value"].is_null() || cleared["value"] == "null", "value clears the field: {:?}", cleared["value"]);
}

#[tokio::test]
async fn deleting_a_done_activity_logs_the_reminder_being_unlinked() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let oid = car["id"].as_i64().unwrap();
    let activity: serde_json::Value = app.client.post(app.url(&format!("/objects/{oid}/activities")))
        .json(&json!({ "date": "2026-09-01", "category": "maintenance", "title": "Oil" }))
        .send().await.unwrap().json().await.unwrap();
    let reminder: serde_json::Value = app.client.post(app.url(&format!("/objects/{oid}/reminders")))
        .json(&json!({ "title": "Oil", "due_date": "2026-09-01" }))
        .send().await.unwrap().json().await.unwrap();
    let res = app.client.post(app.url(&format!("/reminders/{}/done", reminder["id"])))
        .json(&json!({ "activity_id": activity["id"] })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let before: serde_json::Value = app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap().json().await.unwrap();
    let since = before["next_seq"].as_i64().unwrap();
    let epoch = before["epoch"].as_str().unwrap().to_string();
    let res = app.client.delete(app.url(&format!("/activities/{}", activity["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    let after: serde_json::Value = app.client.get(app.url(&format!("/sync/pull?since={since}&epoch={epoch}"))).send().await.unwrap().json().await.unwrap();
    let unlinked = after["changes"].as_array().unwrap().iter().find(|c|
        c["entity"] == "reminder" && c["op"] == "set" && c["field"] == "done_activity_id");
    assert!(unlinked.is_some(), "the unlink is in the feed");
    assert_eq!(unlinked.unwrap()["entity_id"], reminder["id"]);
}

/// The server's own id and uuid for an object, as sync addresses it.
async fn object_uuid(app: &common::TestApp, id: i64) -> String {
    client_uuid(&app.state.db, "objects", id).await
}

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
    let id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    let body: serde_json::Value = app.client.get(app.url("/sync/bootstrap")).send().await.unwrap().json().await.unwrap();
    assert_eq!(body["objects"][0]["tags"], "[\"Lease\",\"winter\"]", "{body}");

    let res = app.client.patch(app.url(&format!("/objects/{id}")))
        .json(&json!({ "name": "Golf", "type": "car", "description": "", "tags": ["Lease", "winter", "Tax"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap().json().await.unwrap();
    let row = body["changes"].as_array().unwrap().iter()
        .find(|c| c["op"] == "set" && c["field"] == "tags")
        .unwrap_or_else(|| panic!("no set/tags row in {body}"));
    assert_eq!(row["value"], json!("[\"Lease\",\"winter\",\"Tax\"]").to_string());
}

#[tokio::test]
async fn a_pushed_set_op_on_tags_is_normalised() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let uuid = object_uuid(&app, id).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-tags", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "tags", "value": "[\" Winter \",\"winter\",\"Lease\"]",
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let obj = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(obj["tags"], json!(["Winter", "Lease"]));

    // Other devices must learn the stored tags, not the spelling this device happened to send.
    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap().json().await.unwrap();
    let row = body["changes"].as_array().unwrap().iter()
        .find(|c| c["op"] == "set" && c["field"] == "tags")
        .unwrap_or_else(|| panic!("no set/tags row in {body}"));
    assert_eq!(row["value"], json!("[\"Winter\",\"Lease\"]").to_string());

    // Entries take the same field.
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2026-03-01", "category": "repair", "title": "Tyres", "notes": "" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let activity_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-act-tags", "entity": "activity", "entity_uuid": activity_uuid,
        "op": "set", "field": "tags", "value": "[\"tax 2026\",\"  TAX   2026 \"]",
        "edited_at": after_now(60), "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "accepted");
    let act = app.get_json(&format!("/activities/{activity_id}")).await;
    assert_eq!(act["tags"], json!(["tax 2026"]));
}

#[tokio::test]
async fn a_pushed_set_op_with_too_many_tags_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "car", "description": "", "tags": ["Lease"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();
    let uuid = object_uuid(&app, id).await;
    let before: serde_json::Value = app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap().json().await.unwrap();
    let since = before["next_seq"].as_i64().unwrap();
    let epoch = before["epoch"].as_str().unwrap().to_string();

    let many: Vec<String> = (0..11).map(|i| format!("t{i}")).collect();
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "op-many", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "tags", "value": serde_json::to_string(&many).unwrap(),
          "edited_at": after_now(60), "device_id": "phone" },
        { "client_op_id": "op-not-json", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "tags", "value": "Lease, winter",
          "edited_at": after_now(60), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "the batch must not 500: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert!(body["results"][0]["reason"].as_str().unwrap().contains("10"), "{body}");
    assert_eq!(body["results"][1]["outcome"], "rejected", "{body}");

    let obj = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(obj["tags"], json!(["Lease"]));

    // A rejected op must not leak into the change feed either.
    let body: serde_json::Value = app.client.get(app.url(&format!("/sync/pull?since={since}&epoch={epoch}")))
        .send().await.unwrap().json().await.unwrap();
    // Either as sent or double-encoded like any logged string value.
    let raw = [serde_json::to_string(&many).unwrap(), "Lease, winter".to_string()];
    let rejected: Vec<String> = raw.iter().flat_map(|r| [r.clone(), json!(r).to_string()]).collect();
    let leaked = body["changes"].as_array().unwrap().iter().any(|c| {
        c["op"] == "set" && c["field"] == "tags" && c["value"].as_str().is_some_and(|v| rejected.iter().any(|r| r == v))
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
    assert!(body["ids"][&type_uuid].is_i64(), "the push names the new type's id: {body}");

    let snapshot = app.get_json("/sync/bootstrap").await;
    let types = snapshot["object_types"].as_array().unwrap_or_else(|| panic!("no object_types in {snapshot}"));
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
    let res = anna.post(app.url("/sync/push")).json(&push_body(json!([
        type_create_op("anna-type", &type_uuid, "Mine"),
    ]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
}

#[tokio::test]
async fn a_set_op_renaming_a_type_is_validated() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let scooter = app.post_json("/types", &json!({ "name": "E-scooter", "icon": "e-bike", "categories": ["repair"] })).await;
    app.post_json("/types", &json!({ "name": "Boat", "icon": "box", "categories": ["repair"] })).await;
    let uuid = scooter["client_uuid"].as_str().unwrap().to_string();
    let set = |op_id: &str, field: &str, value: serde_json::Value, secs: i64| json!({
        "client_op_id": op_id, "entity": "object_type", "entity_uuid": uuid, "op": "set",
        "field": field, "value": value, "edited_at": after_now(secs), "device_id": "phone"
    });

    let res = app.push_raw(&push_body(json!([
        set("rename", "name", json!("  Kickscooter "), 60),
        set("bad-icon", "icon", json!("rocket"), 61),
        set("taken", "name", json!("boat"), 62),
        set("cats", "categories", json!("[\"fuel\",\"fuel\"]"), 63),
        set("bad-cats", "categories", json!("[\"sailing\"]"), 64),
        set("no-name", "name", json!(null), 65),
        set("unit", "counter_unit", json!(null), 66),
    ]))).await;
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    let outcomes: Vec<&str> = body["results"].as_array().unwrap().iter().map(|r| r["outcome"].as_str().unwrap()).collect();
    assert_eq!(outcomes, ["accepted", "rejected", "rejected", "accepted", "rejected", "rejected", "accepted"], "{body}");
    assert!(body["results"][1]["reason"].as_str().unwrap().contains("icon"), "{body}");

    let stored = app.get_json("/types").await;
    let stored = stored.as_array().unwrap().iter().find(|t| t["client_uuid"] == uuid).unwrap().clone();
    assert_eq!(stored["name"], "Kickscooter");
    assert_eq!(stored["icon"], "e-bike");
    assert_eq!(stored["categories"], json!(["fuel", "other"]));
    assert_eq!(stored["counter_unit"], serde_json::Value::Null);

    // The feed carries what was stored, not the device's spelling.
    let feed = app.pull(0).await;
    let logged: Vec<(String, String)> = feed["changes"].as_array().unwrap().iter()
        .filter(|c| c["entity"] == "object_type" && c["op"] == "set")
        .map(|c| (c["field"].as_str().unwrap().to_string(), c["value"].as_str().unwrap_or("null").to_string()))
        .collect();
    assert!(logged.contains(&("name".into(), json!("Kickscooter").to_string())), "{feed}");
    assert!(logged.contains(&("categories".into(), json!("[\"fuel\",\"other\"]").to_string())), "{feed}");
    assert!(!logged.iter().any(|(f, _)| f == "icon"), "a rejected set must not reach the feed: {feed}");

    // Another user's type is not theirs to rename.
    let anna = app.create_user_client("anna", "password123").await;
    let res = anna.post(app.url("/sync/push")).json(&push_body(json!([set("anna", "name", json!("Stolen"), 70)])))
        .send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
}

#[tokio::test]
async fn deleting_a_type_in_use_is_rejected_over_sync() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let scooter = app.post_json("/types", &json!({ "name": "E-scooter", "icon": "e-bike", "categories": ["repair"] })).await;
    let uuid = scooter["client_uuid"].as_str().unwrap().to_string();
    let object = app.post_json("/objects", &json!({ "name": "Kick", "type": scooter["key"] })).await;
    let delete = |op_id: &str, secs: i64| push_body(json!([{
        "client_op_id": op_id, "entity": "object_type", "entity_uuid": uuid, "op": "delete",
        "edited_at": after_now(secs), "device_id": "phone"
    }]));

    let body: serde_json::Value = app.push_raw(&delete("del-1", 60)).await.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert!(body["results"][0]["reason"].as_str().unwrap().contains("in use"), "{body}");
    assert_eq!(app.get_json("/types").await.as_array().unwrap().len(), 1);
    assert_eq!(app.count_changes_of("del-1").await, 0);

    app.delete_object(&object).await;
    let body: serde_json::Value = app.push_raw(&delete("del-2", 61)).await.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    assert_eq!(app.get_json("/types").await, json!([]));
    let snapshot = app.get_json("/sync/bootstrap").await;
    assert_eq!(snapshot["object_types"], json!([]));
}

#[tokio::test]
async fn rest_type_writes_appear_in_the_change_feed() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let scooter = app.post_json("/types", &json!({ "name": "E-scooter", "icon": "e-bike", "categories": ["repair"] })).await;
    let (id, uuid) = (scooter["id"].as_i64().unwrap(), scooter["client_uuid"].as_str().unwrap().to_string());
    let res = app.client.patch(app.url(&format!("/types/{id}")))
        .json(&json!({ "name": "Kickscooter", "icon": "e-bike", "categories": ["repair"], "counter_unit": "km" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
    let res = app.client.delete(app.url(&format!("/types/{id}"))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    let feed = app.pull(0).await;
    let rows: Vec<(String, Option<String>, Option<i64>)> = feed["changes"].as_array().unwrap().iter()
        .filter(|c| c["entity"] == "object_type")
        .map(|c| {
            assert_eq!(c["entity_uuid"], uuid, "{feed}");
            (c["op"].as_str().unwrap().to_string(), c["field"].as_str().map(str::to_string), c["entity_id"].as_i64())
        })
        .collect();
    assert_eq!(rows, [
        ("create".to_string(), None, Some(id)),
        ("set".to_string(), Some("name".to_string()), Some(id)),
        ("set".to_string(), Some("counter_unit".to_string()), Some(id)),
        ("delete".to_string(), None, Some(id)),
    ], "unchanged icon and categories log nothing: {feed}");

    // The REST create stamped the clock, so a stale offline rename loses.
    let body: serde_json::Value = app.push_raw(&push_body(json!([{
        "client_op_id": "stale", "entity": "object_type", "entity_uuid": uuid, "op": "set",
        "field": "icon", "value": "box", "edited_at": before_now(3600), "device_id": "phone"
    }]))).await.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "superseded", "{body}");
}
