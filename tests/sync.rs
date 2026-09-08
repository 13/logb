mod common;
use reqwest::multipart::{Form, Part};
use serde_json::json;

#[tokio::test]
async fn every_created_row_gets_a_client_uuid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(uuid.len(), 36, "a v4 uuid in hyphenated form: {uuid}");

    let deleted: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM objects WHERE id = ?")
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

    let object_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = ?")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(object_rows, 1, "the row survives; only deleted_at is set");

    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM objects WHERE id = ? AND deleted_at IS NULL")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(live, 0, "the object is tombstoned");

    let live_children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activities WHERE object_id = ? AND deleted_at IS NULL")
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

/// Task 2's read-path filters (`/search`, `/export`, `/insights`, the reminder digest) are the
/// bulk of that change and are each exercised separately elsewhere; this is the one place that
/// checks all four agree a deleted object's activity, reminder and attachment are gone, not
/// just the object itself.
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
    assert!(logby::notify::collect(&app.state).await.unwrap().is_some(), "the reminder really is due");

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
        logby::notify::collect(&app.state).await.unwrap().is_none(),
        "a tombstoned reminder must not appear in the digest"
    );

    // The attachment's file, reachable only through it, is also unreadable.
    assert_eq!(app.client.get(app.url(&format!("/files/{file_id}"))).send().await.unwrap().status(), 404);
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
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-1", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Golf VII",
        "edited_at": "2026-02-01T10:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted");
    assert!(body["server_time"].is_string());

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
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
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let newer = json!([{
        "client_op_id": "op-new", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Newer",
        "edited_at": "2026-02-02T00:00:00Z", "device_id": "phone"
    }]);
    let older = json!([{
        "client_op_id": "op-old", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Older",
        "edited_at": "2026-02-01T00:00:00Z", "device_id": "phone"
    }]);
    app.client.post(app.url("/sync/push")).json(&push_body(newer)).send().await.unwrap();
    let res = app.client.post(app.url("/sync/push")).json(&push_body(older)).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();

    assert_eq!(body["results"][0]["outcome"], "superseded");
    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
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
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let batch = push_body(json!([{
        "client_op_id": "op-same", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Once",
        "edited_at": "2026-02-01T00:00:00Z", "device_id": "phone"
    }]));
    for _ in 0..2 {
        let res = app.client.post(app.url("/sync/push")).json(&batch).send().await.unwrap();
        assert_eq!(res.status(), 200);
    }
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-same'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1, "the same op id lands exactly once");
}

#[tokio::test]
async fn timestamps_are_compared_chronologically_not_lexically() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    // Whole seconds first, then a value half a second LATER written with a fraction. Compared
    // as raw strings the fractional one loses ('.' < 'Z'), so a lexical rule would keep "Early".
    for (id, value, at) in [
        ("op-whole", "Early", "2026-08-01T00:00:00Z"),
        ("op-frac", "Later", "2026-08-01T00:00:00.500Z"),
    ] {
        let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": id, "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": value,
            "edited_at": at, "device_id": "phone"
        }]))).send().await.unwrap();
        assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    }

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Later", "the chronologically later edit must win");

    // An offset-form timestamp is the same instant as its Z form and must not re-win.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-offset", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Earlier still",
        "edited_at": "2026-08-01T00:00:00+00:00", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "superseded");

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-junk", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Nonsense",
        "edited_at": "last thursday", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "rejected");

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Later");
}

#[tokio::test]
async fn a_field_outside_the_whitelist_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-evil", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "user_id", "value": 2,
        "edited_at": "2026-02-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");
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
    let their_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(theirs["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = mallory.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-steal", "entity": "object", "entity_uuid": their_uuid,
        "op": "set", "field": "cover_attachment_id", "value": victim_attachment,
        "edited_at": "2026-07-01T00:00:00Z", "device_id": "mallory-phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");

    let cover: Option<i64> = sqlx::query_scalar(
        "SELECT cover_attachment_id FROM objects WHERE client_uuid = ?")
        .bind(&their_uuid).fetch_one(&app.state.db).await.unwrap();
    assert!(cover.is_none(), "the cross-account reference must not have landed");
}

#[tokio::test]
async fn one_user_cannot_push_at_another_users_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
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

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Golf", "an unrelated user changed nothing");
}

#[tokio::test]
async fn results_stay_in_the_order_the_ops_were_sent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    // A batch that mixes ops rejected at different stages -- an unparseable timestamp, a field
    // off the whitelist -- with ones that land. `results[i]` must still describe `ops[i]`, so a
    // client can line the two lists up by position and not only by `client_op_id`.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "ord-1", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "name", "value": "First",
          "edited_at": "2026-03-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "ord-2", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "category", "value": "car",
          "edited_at": "not a timestamp", "device_id": "phone" },
        { "client_op_id": "ord-3", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "description", "value": "Mine",
          "edited_at": "2026-03-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "ord-4", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "user_id", "value": 2,
          "edited_at": "2026-03-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "ord-5", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "name", "value": "Superseded by ord-1",
          "edited_at": "2026-01-01T00:00:00Z", "device_id": "phone" }
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
    let my_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(mine["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let other = app.create_user_client("mallory", "another password").await;
    let theirs = app.create_object(&other, "Bike", None).await;
    let their_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
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
            "edited_at": "2026-05-01T00:00:00Z", "device_id": "phone"
        }]))).send().await.unwrap();
        assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    }

    for (uuid, expected) in [(&my_uuid, "Golf VII"), (&their_uuid, "Brompton")] {
        let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
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
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
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
        "edited_at": "2026-06-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "accepted");

    // A foreign key holds an id or nothing. SQLite would happily store this string in the
    // integer column, so the ownership check -- which only looks at integers -- must not be
    // the only thing standing between a junk value and the write.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-junk-fk", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "cover_attachment_id", "value": "not-an-id",
        "edited_at": "2026-06-02T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");

    let cover: Option<i64> = sqlx::query_scalar(
        "SELECT cover_attachment_id FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(cover, Some(attachment_id), "the column must be untouched by the rejected op");

    // Null is still how a client clears the reference, and must not be caught by the above.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-clear-fk", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "cover_attachment_id", "value": null,
        "edited_at": "2026-06-03T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    let cover: Option<i64> = sqlx::query_scalar(
        "SELECT cover_attachment_id FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert!(cover.is_none(), "null clears the reference");
}

/// The uuid a client knows a row by. Table names here are literals in this file, never input.
async fn client_uuid(db: &sqlx::SqlitePool, table: &str, id: i64) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT client_uuid FROM {table} WHERE id = ?"
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
        let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM objects WHERE id = ?")
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

    let title: String = sqlx::query_scalar("SELECT title FROM activities WHERE id = ?")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(title, "Timing belt", "another account's activity is untouched");
    let title: String = sqlx::query_scalar("SELECT title FROM reminders WHERE id = ?")
        .bind(reminder_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(title, "Service", "another account's reminder is untouched");
    let caption: String = sqlx::query_scalar("SELECT caption FROM attachments WHERE id = ?")
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
          "edited_at": "2026-02-01T00:00:00Z", "device_id": "ben-phone" },
        { "client_op_id": "mine-rem", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "title", "value": "Service booked",
          "edited_at": "2026-02-01T00:00:00Z", "device_id": "ben-phone" },
        { "client_op_id": "mine-att", "entity": "attachment", "entity_uuid": attachment_uuid,
          "op": "set", "field": "caption", "value": "The old belt",
          "edited_at": "2026-02-01T00:00:00Z", "device_id": "ben-phone" }
    ]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    for i in 0..3 {
        assert_eq!(body["results"][i]["outcome"], "accepted", "{body}");
    }

    let title: String = sqlx::query_scalar("SELECT title FROM activities WHERE id = ?")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(title, "Timing belt done", "the owner's own write must land");
    let title: String = sqlx::query_scalar("SELECT title FROM reminders WHERE id = ?")
        .bind(reminder_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(title, "Service booked", "the owner's own write must land");
    let caption: String = sqlx::query_scalar("SELECT caption FROM attachments WHERE id = ?")
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
        "op": "delete", "edited_at": "2026-04-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM activities WHERE id = ?")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(rows, 1, "the row survives; only deleted_at is set");
    let deleted: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM activities WHERE id = ?")
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
          "edited_at": "2026-04-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "bad-null", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "name", "value": null,
          "edited_at": "2026-04-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "bad-check", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "counter_unit", "value": "furlongs",
          "edited_at": "2026-04-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "ok-after", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "category", "value": "boat",
          "edited_at": "2026-04-01T00:00:00Z", "device_id": "phone" }
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
        "SELECT name, category, counter_unit, description FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(
        row,
        ("Golf".into(), "boat".into(), Some("km".into()), "Mine".into()),
        "accepted ops committed; rejected ops wrote nothing"
    );

    let logged: Vec<String> = sqlx::query_scalar(
        "SELECT client_op_id FROM changes ORDER BY seq")
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
          "edited_at": "2026-05-02T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "num-huge", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "counter_value", "value": 100000000000000000000000_i128 as f64,
          "edited_at": "2026-05-02T00:00:00Z", "device_id": "phone" }
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
        "SELECT purchase_price_cents FROM objects WHERE client_uuid = ?")
        .bind(&object_uuid).fetch_one(&app.state.db).await.unwrap();
    assert!(price.is_none(), "the row still holds what the REST create put there");
    let counter: Option<i64> = sqlx::query_scalar(
        "SELECT counter_value FROM activities WHERE client_uuid = ?")
        .bind(&activity_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(counter, Some(1000), "the good value the REST create wrote must survive");

    // The repair: the client resends the value it always had, carrying its ORIGINAL edited_at,
    // which is EARLIER than the rejected op's. That only wins if the rejected op left no
    // `field_clock` row behind.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "num-repair", "entity": "object", "entity_uuid": object_uuid,
        "op": "set", "field": "purchase_price_cents", "value": 1250,
        "edited_at": "2026-05-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "the client can still repair: {body}");
    let price: Option<i64> = sqlx::query_scalar(
        "SELECT purchase_price_cents FROM objects WHERE client_uuid = ?")
        .bind(&object_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(price, Some(1250));

    // An integer is still an ordinary accepted value.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "num-int", "entity": "activity", "entity_uuid": activity_uuid,
        "op": "set", "field": "cost_cents", "value": 9900,
        "edited_at": "2026-05-03T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    let cost: Option<i64> = sqlx::query_scalar(
        "SELECT cost_cents FROM activities WHERE client_uuid = ?")
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
          "edited_at": "2026-05-02T00:00:00Z", "device_id": "phone" },
        // And the milder direction, which corrupts silently: SQLite stores `true` in a TEXT
        // column as '1', so the object's name would have become the string "1".
        { "client_op_id": "type-bool-into-text", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "name", "value": true,
          "edited_at": "2026-05-02T00:00:00Z", "device_id": "phone" }
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
    let stored: (String, Option<i64>) = sqlx::query_as(
        "SELECT typeof(counter_value), counter_value FROM activities WHERE id = ?")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(stored, ("integer".into(), Some(1000)), "the integer column is still an integer");
    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE id = ?")
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
        "edited_at": "2026-05-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "the rejected op left no clock: {body}");
    let counter: Option<i64> = sqlx::query_scalar("SELECT counter_value FROM activities WHERE id = ?")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(counter, Some(2000));
}
