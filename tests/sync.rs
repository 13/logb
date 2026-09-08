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
