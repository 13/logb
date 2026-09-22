//! Fixtures shared by more than one theme module.
use super::common;
use reqwest::multipart::{Form, Part};
use serde_json::json;

/// `field_clock` stamp would have started failing the moment "now" caught up to the literal.
/// An offset from the clock the test actually runs against cannot expire.
pub fn after_now(secs: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(secs))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The mirror of `after_now`, for an op that must lose last-write-wins against something
/// stamped at real "now" -- e.g. a phone op standing in for "before the browser's edit".
pub fn before_now(secs: i64) -> String {
    after_now(-secs)
}

pub fn png() -> Vec<u8> {
    let img = image::DynamicImage::new_rgb8(8, 8);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// An op batch as the wire format expects it.
pub fn push_body(ops: serde_json::Value) -> serde_json::Value {
    json!({ "ops": ops })
}

/// The uuid a client knows a row by. Table names here are literals in this file, never input.
pub async fn client_uuid(db: &sqlx::AnyPool, table: &str, id: i64) -> String {
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
pub async fn object_with_children(
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

    let res = client
        .post(app.url(&format!("/objects/{object_id}/reminders")))
        .json(&json!({ "title": "Service", "due_date": "2026-09-01" }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        201,
        "create reminder: {}",
        res.text().await.unwrap()
    );
    let reminder_id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    let res = client
        .post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(
            Form::new().part(
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

    (object_id, activity_id, reminder_id, attachment_id)
}

/// The server's own id and uuid for an object, as sync addresses it.
pub async fn object_uuid(app: &common::TestApp, id: i64) -> String {
    client_uuid(&app.state.db, "objects", id).await
}
