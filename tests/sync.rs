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
        "edited_at": "2031-02-01T10:00:00Z", "device_id": "phone"
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
        "edited_at": "2031-02-02T00:00:00Z", "device_id": "phone"
    }]);
    let older = json!([{
        "client_op_id": "op-old", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Older",
        "edited_at": "2031-02-01T00:00:00Z", "device_id": "phone"
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
        "edited_at": "2031-02-01T00:00:00Z", "device_id": "phone"
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
        ("op-whole", "Early", "2031-08-01T00:00:00Z"),
        ("op-frac", "Later", "2031-08-01T00:00:00.500Z"),
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
        "edited_at": "2031-08-01T00:00:00+00:00", "device_id": "phone"
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
        "edited_at": "2031-02-01T00:00:00Z", "device_id": "phone"
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
        "edited_at": "2031-07-01T00:00:00Z", "device_id": "mallory-phone"
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
          "edited_at": "2031-03-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "ord-2", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "category", "value": "car",
          "edited_at": "not a timestamp", "device_id": "phone" },
        { "client_op_id": "ord-3", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "description", "value": "Mine",
          "edited_at": "2031-03-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "ord-4", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "user_id", "value": 2,
          "edited_at": "2031-03-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "ord-5", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "name", "value": "Superseded by ord-1",
          "edited_at": "2031-01-01T00:00:00Z", "device_id": "phone" }
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
            "edited_at": "2031-05-01T00:00:00Z", "device_id": "phone"
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
        "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "accepted");

    // A foreign key holds an id or nothing. SQLite would happily store this string in the
    // integer column, so the ownership check -- which only looks at integers -- must not be
    // the only thing standing between a junk value and the write.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-junk-fk", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "cover_attachment_id", "value": "not-an-id",
        "edited_at": "2031-06-02T00:00:00Z", "device_id": "phone"
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
        "edited_at": "2031-06-03T00:00:00Z", "device_id": "phone"
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
          "edited_at": "2031-02-01T00:00:00Z", "device_id": "ben-phone" },
        { "client_op_id": "mine-rem", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "title", "value": "Service booked",
          "edited_at": "2031-02-01T00:00:00Z", "device_id": "ben-phone" },
        { "client_op_id": "mine-att", "entity": "attachment", "entity_uuid": attachment_uuid,
          "op": "set", "field": "caption", "value": "The old belt",
          "edited_at": "2031-02-01T00:00:00Z", "device_id": "ben-phone" }
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
        "op": "delete", "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone"
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
          "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "bad-null", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "name", "value": null,
          "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "bad-check", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "counter_unit", "value": "furlongs",
          "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "ok-after", "entity": "object", "entity_uuid": uuid,
          "op": "set", "field": "category", "value": "boat",
          "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone" }
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
          "edited_at": "2031-05-02T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "num-huge", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "counter_value", "value": 100000000000000000000000_i128 as f64,
          "edited_at": "2031-05-02T00:00:00Z", "device_id": "phone" }
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
        "edited_at": "2031-05-01T00:00:00Z", "device_id": "phone"
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
        "edited_at": "2031-05-03T00:00:00Z", "device_id": "phone"
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
          "edited_at": "2031-05-02T00:00:00Z", "device_id": "phone" },
        // And the milder direction, which corrupts silently: SQLite stores `true` in a TEXT
        // column as '1', so the object's name would have become the string "1".
        { "client_op_id": "type-bool-into-text", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "name", "value": true,
          "edited_at": "2031-05-02T00:00:00Z", "device_id": "phone" }
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
        "edited_at": "2031-05-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "the rejected op left no clock: {body}");
    let counter: Option<i64> = sqlx::query_scalar("SELECT counter_value FROM activities WHERE id = ?")
        .bind(activity_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(counter, Some(2000));
}

#[tokio::test]
async fn pull_returns_ops_after_the_cursor_and_advances_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    for (n, name) in [("op-a", "First"), ("op-b", "Second")] {
        app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": n, "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": name,
            "edited_at": format!("2031-03-0{}T00:00:00Z", if n == "op-a" { 1 } else { 2 }),
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

    let body: serde_json::Value = app.client.get(app.url(&format!("/sync/pull?since={next}")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 0, "the cursor is exhausted");
}

#[tokio::test]
async fn pull_pages_and_reports_incompleteness() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    for i in 0..3 {
        app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": format!("op-{i}"), "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "description", "value": format!("note {i}"),
            "edited_at": format!("2031-04-0{}T00:00:00Z", i + 1), "device_id": "phone"
        }]))).send().await.unwrap();
    }

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0&limit=2"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 2);
    assert_eq!(body["complete"], false, "more remains behind the page");
}

#[tokio::test]
async fn pull_never_leaks_another_users_changes() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-ben", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Ben's",
        "edited_at": "2031-05-01T00:00:00Z", "device_id": "phone"
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
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-kept", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Kept",
        "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();

    // Simulate a purge having removed everything before this row -- including the object's
    // own `create`, which is in the log too now (task 9), or the horizon would still read as
    // the create row's untouched seq and this cursor would look current rather than stale.
    sqlx::query("DELETE FROM changes WHERE client_op_id != 'op-kept'")
        .execute(&app.state.db).await.unwrap();
    sqlx::query("UPDATE changes SET seq = 500 WHERE client_op_id = 'op-kept'")
        .execute(&app.state.db).await.unwrap();

    let res = app.client.get(app.url("/sync/pull?since=1")).send().await.unwrap();
    assert_eq!(res.status(), 410, "a stale cursor must be told to re-bootstrap");
}

#[tokio::test]
async fn a_cursor_against_an_emptied_log_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // A non-zero cursor can only have come from ops that existed, so an empty log means they
    // were purged. Answering 200 here would let the client believe it is current forever.
    let res = app.client.get(app.url("/sync/pull?since=7")).send().await.unwrap();
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

    // The cursor is immediately usable.
    let seq = body["seq"].as_i64().unwrap();
    let res = app.client.get(app.url(&format!("/sync/pull?since={seq}"))).send().await.unwrap();
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

#[tokio::test]
async fn purge_drops_old_log_rows_and_old_tombstones() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();

    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-ancient", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Ancient",
        "edited_at": "2031-01-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();

    // Backdate both the log row and a tombstone well past any sane window.
    sqlx::query("UPDATE changes SET applied_at = '2000-01-01T00:00:00Z'")
        .execute(&app.state.db).await.unwrap();
    sqlx::query("UPDATE objects SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = ?")
        .bind(object_id).execute(&app.state.db).await.unwrap();

    let removed = logby::sync::feed::purge(&app.state, 90).await.unwrap();
    // The blanket backdate above ages out every `changes` row for this user, including the
    // object's own `create` (task 9 logs REST creates too), not only the pushed `set`.
    assert_eq!(removed, 2, "the ancient log rows went");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(rows, 0);

    let objects: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = ?")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(objects, 0, "an expired tombstone is finally a real delete");
}

#[tokio::test]
async fn purge_keeps_recent_history() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-fresh", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Fresh",
        "edited_at": "2031-01-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();

    assert_eq!(logby::sync::feed::purge(&app.state, 90).await.unwrap(), 0);
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
        "SELECT f.sha256 FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = ?")
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
    logby::sync::feed::purge(&app.state, 90).await.unwrap();

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
        "op": "delete", "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    for (table, id) in
        [("activities", activity_id), ("reminders", reminder_id), ("attachments", attachment_id)]
    {
        let deleted: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT deleted_at FROM {table} WHERE id = ?"
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
            "SELECT count(*) FROM changes WHERE entity = ? AND entity_uuid = ? AND op = 'delete'")
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
        "op": "delete", "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    let deleted: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM attachments WHERE id = ?")
            .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    assert!(deleted.is_some(), "the activity's attachment must be tombstoned with it");

    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE entity = 'attachment' AND entity_uuid = ? \
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
        "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "set cover failed: {}", res.text().await.unwrap());
    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE id = ?")
            .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(cover, Some(attachment_id), "fixture setup: the cover must be set before deletion");

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-attachment", "entity": "attachment", "entity_uuid": attachment_uuid,
        "op": "delete", "edited_at": "2031-04-02T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE id = ?")
            .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert!(cover.is_none(), "a pushed attachment delete must clear the object's cover, same as REST");
}

/// `apply_op`'s own-row tombstone `UPDATE` set only `deleted_at`, while the REST delete
/// handlers also stamp `updated_at`. Only `objects` and `activities` carry that column
/// (`reminders`, `attachments` and `files` do not -- see `migrations/0001_init.sql`), so this
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
    sqlx::query("UPDATE objects SET updated_at = ? WHERE id = ?")
        .bind(stale).bind(object_id).execute(&app.state.db).await.unwrap();
    sqlx::query("UPDATE activities SET updated_at = ? WHERE id = ?")
        .bind(stale).bind(activity_id).execute(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-object", "entity": "object", "entity_uuid": object_uuid,
        "op": "delete", "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    let object_updated: String = sqlx::query_scalar("SELECT updated_at FROM objects WHERE id = ?")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_ne!(object_updated, stale, "the object's own tombstone must bump updated_at");

    let activity_updated: String =
        sqlx::query_scalar("SELECT updated_at FROM activities WHERE id = ?")
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
    sqlx::query("UPDATE activities SET updated_at = ? WHERE id = ?")
        .bind(stale).bind(activity_id).execute(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-activity", "entity": "activity", "entity_uuid": activity_uuid,
        "op": "delete", "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    let activity_updated: String =
        sqlx::query_scalar("SELECT updated_at FROM activities WHERE id = ?")
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
        "SELECT f.sha256 FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = ?")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    let blob = app.state.storage.blob_path(&sha);
    assert!(blob.exists(), "the fixture's upload landed on disk");

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-object", "entity": "object", "entity_uuid": object_uuid,
        "op": "delete", "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

    // Only the object's tombstone is aged out. Its children were tombstoned just now, so they
    // are still inside the window and the purge has no business removing them yet -- through
    // the cascade or otherwise.
    sqlx::query("UPDATE objects SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = ?")
        .bind(object_id).execute(&app.state.db).await.unwrap();
    logby::sync::feed::purge(&app.state, 90).await.unwrap();

    for (table, id) in
        [("activities", activity_id), ("reminders", reminder_id), ("attachments", attachment_id)]
    {
        let rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE id = ?"
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
    logby::sync::feed::purge(&app.state, 90).await.unwrap();

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
        let user_id: i64 = sqlx::query_scalar("SELECT user_id FROM objects WHERE id = ?")
            .bind(object_id).fetch_one(&app.state.db).await.unwrap();

        // A live clock the sweep must leave alone, so a run that swept everything -- rather
        // than only the orphan -- would still be caught.
        let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": "op-name", "entity": "object", "entity_uuid": &object_uuid,
            "op": "set", "field": "name", "value": "Renamed",
            "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone"
        }]))).send().await.unwrap();
        assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());

        // A row from before `client_uuid` existed, or from any writer that never set it, in the
        // table under test this iteration -- every column each table's NOT NULL constraints
        // require, and nothing that names `client_uuid`, so it defaults NULL.
        match legacy_table {
            "objects" => {
                sqlx::query(
                    "INSERT INTO objects (user_id, name, category, created_at, updated_at) \
                     VALUES (?, 'Legacy', 'car', '2026-03-05T00:00:00Z', '2026-03-05T00:00:00Z')")
                    .bind(user_id).execute(&app.state.db).await.unwrap();
            }
            "activities" => {
                sqlx::query(
                    "INSERT INTO activities \
                     (object_id, date, category, title, notes, created_at, updated_at) \
                     VALUES (?, '2026-03-05', 'other', 'Legacy', '', \
                             '2026-03-05T00:00:00Z', '2026-03-05T00:00:00Z')")
                    .bind(object_id).execute(&app.state.db).await.unwrap();
            }
            "reminders" => {
                sqlx::query(
                    "INSERT INTO reminders (object_id, title, due_date, created_at) \
                     VALUES (?, 'Legacy', '2026-09-01', '2026-03-05T00:00:00Z')")
                    .bind(object_id).execute(&app.state.db).await.unwrap();
            }
            "attachments" => {
                let file_id: i64 = sqlx::query_scalar(
                    "INSERT INTO files (user_id, sha256, original_name, mime, size, created_at) \
                     VALUES (?, 'deadbeef', 'legacy.png', 'image/png', 1, '2026-03-05T00:00:00Z') \
                     RETURNING id")
                    .bind(user_id).fetch_one(&app.state.db).await.unwrap();
                sqlx::query(
                    "INSERT INTO attachments (object_id, file_id, kind, created_at) \
                     VALUES (?, ?, 'photo', '2026-03-05T00:00:00Z')")
                    .bind(object_id).bind(file_id).execute(&app.state.db).await.unwrap();
            }
            "files" => {
                sqlx::query(
                    "INSERT INTO files (user_id, sha256, original_name, mime, size, created_at) \
                     VALUES (?, 'deadbeef', 'legacy.png', 'image/png', 1, '2026-03-05T00:00:00Z')")
                    .bind(user_id).execute(&app.state.db).await.unwrap();
            }
            other => unreachable!("not one of the five tables: {other}"),
        }

        // A clock for a uuid no table carries any more: the sweep's whole reason to exist.
        sqlx::query(
            "INSERT INTO field_clock (entity, entity_uuid, field, edited_at, device_id) \
             VALUES ('activity', 'gone-with-the-row', 'title', '2026-01-01T00:00:00Z', 'phone')")
            .execute(&app.state.db).await.unwrap();

        logby::sync::feed::purge(&app.state, 90).await.unwrap();

        let orphans: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM field_clock WHERE entity_uuid = 'gone-with-the-row'")
            .fetch_one(&app.state.db).await.unwrap();
        assert_eq!(
            orphans, 0,
            "the orphaned clock must be swept even beside a NULL client_uuid in {legacy_table}"
        );

        let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM field_clock WHERE entity_uuid = ?")
            .bind(&object_uuid).fetch_one(&app.state.db).await.unwrap();
        // 9, not 1: the object's own REST `create` stamps every field in `Entity::Object`'s
        // whitelist (task 9), and the pushed `set` above only overwrites `name`'s entry rather
        // than adding a tenth. All 9 must survive the sweep untouched.
        assert_eq!(
            kept, 9,
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
        "SELECT f.client_uuid FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = ?")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-del-file", "entity": "file", "entity_uuid": file_uuid,
        "op": "delete", "edited_at": "2031-04-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");

    let deleted: Option<String> = sqlx::query_scalar(
        "SELECT deleted_at FROM files WHERE client_uuid = ?")
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
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-obj-category", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "category", "value": "",
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-obj-date", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "purchase_date", "value": "not-a-date",
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-obj-price", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "purchase_price_cents", "value": -999,
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-act-date", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "date", "value": "not-a-date",
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-act-title", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "title", "value": "",
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-act-cost", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "cost_cents", "value": -1,
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-act-counter", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "counter_value", "value": -1,
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-act-qty", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "quantity_milli", "value": -1,
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-rem-title", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "title", "value": "",
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-rem-date", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "due_date", "value": "not-a-date",
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-rem-due-counter", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "due_counter", "value": -1,
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-rem-repeat-months", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "repeat_months", "value": 0,
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "v-rem-repeat-counter", "entity": "reminder", "entity_uuid": reminder_uuid,
          "op": "set", "field": "repeat_counter", "value": 0,
          "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone" }
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
    let (name, category, purchase_date, price): (String, String, Option<String>, Option<i64>) =
        sqlx::query_as(
            "SELECT name, category, purchase_date, purchase_price_cents FROM objects WHERE client_uuid = ?")
            .bind(&object_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!((name.as_str(), category.as_str(), purchase_date, price), ("Golf", "car", None, None));

    let (date, title, cost, counter, qty): (String, String, Option<i64>, Option<i64>, Option<i64>) =
        sqlx::query_as(
            "SELECT date, title, cost_cents, counter_value, quantity_milli FROM activities WHERE client_uuid = ?")
            .bind(&activity_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(
        (date.as_str(), title.as_str(), cost, counter, qty),
        ("2026-01-01", "Timing belt", Some(5000), Some(1000), None)
    );

    let (rtitle, rdue_date, rdue_counter, rrepeat_months, rrepeat_counter):
        (String, Option<String>, Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT title, due_date, due_counter, repeat_months, repeat_counter FROM reminders WHERE client_uuid = ?")
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
          "edited_at": "2031-06-02T00:00:00Z", "device_id": "phone" },
        { "client_op_id": "junk-delete", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "delete", "field": "title", "value": "should not be stored either",
          "edited_at": "2031-06-02T00:00:01Z", "device_id": "phone" }
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
        "edited_at": "2031-06-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "set cover failed: {}", res.text().await.unwrap());

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-kind-away", "entity": "attachment", "entity_uuid": attachment_uuid,
        "op": "set", "field": "kind", "value": "document",
        "edited_at": "2031-06-02T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");

    let kind: String = sqlx::query_scalar("SELECT kind FROM attachments WHERE id = ?")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(kind, "photo", "the cover's kind must not change while it is still the cover");
    let cover: Option<i64> = sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE id = ?")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(cover, Some(attachment_id), "the cover pointer must be unaffected");

    // Once it is no longer the cover, the same edit is an ordinary accepted write.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-clear-cover", "entity": "object", "entity_uuid": object_uuid,
        "op": "set", "field": "cover_attachment_id", "value": null,
        "edited_at": "2031-06-03T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-kind-ok", "entity": "attachment", "entity_uuid": attachment_uuid,
        "op": "set", "field": "kind", "value": "document",
        "edited_at": "2031-06-04T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    let kind: String = sqlx::query_scalar("SELECT kind FROM attachments WHERE id = ?")
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
/// (2032) purely so it clears the clock `record_create` already stamped at real "now" when
/// the object was made, and is accepted -- establishing that the browser's later overwrite
/// really is competing against a *newer*-looking stamp, not an empty one. The second phone op
/// is dated in early 2026, safely before whatever "now" this test runs at, standing in for
/// "before the browser's edit" without depending on wall-clock timing precision.
#[tokio::test]
async fn a_rest_edit_beats_a_sync_op_stamped_before_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(id).fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "phone-op-1", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Phone Golf",
        "edited_at": "2032-01-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "accepted");

    // The browser edits the same field over REST. Every other field is resent unchanged so
    // only `name` moves. This unconditionally overwrites `field_clock` to the real current
    // instant, regardless of the phone's on-paper-later 2032 stamp -- a REST write always
    // reflects what just happened.
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Browser Golf", "category": "car", "counter_unit": "km", "description": "",
        "purchase_date": null, "purchase_price_cents": null
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "phone-op-2", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Late Phone Golf",
        "edited_at": "2026-01-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "superseded", "{body}");

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE id = ?")
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
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(id).fetch_one(&app.state.db).await.unwrap();

    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf VII", "category": "car", "counter_unit": "km", "description": "",
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
        "name": "Golf", "category": "car", "counter_unit": "km", "description": "",
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
            "SELECT op, field FROM changes WHERE entity_uuid = ? AND op = 'delete'")
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
