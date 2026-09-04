mod common;
use reqwest::multipart::{Form, Part};
use serde_json::json;

/// Zips a single `data.json` entry containing `data`, as a real export archive would.
fn zip_data_json(data: &serde_json::Value) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut cursor);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file("data.json", opts).unwrap();
        std::io::Write::write_all(&mut w, serde_json::to_vec(data).unwrap().as_slice()).unwrap();
        w.finish().unwrap();
    }
    cursor.into_inner()
}

/// A minimal version-1 export with a single object and no activities/attachments/reminders,
/// for tests to graft a hostile field onto.
fn base_object() -> serde_json::Value {
    json!({
        "name": "Golf", "category": "car", "counter_unit": "km", "description": "",
        "purchase_date": null, "purchase_price_cents": null, "archived_at": null,
        "created_at": "2024-01-01T00:00:00Z", "cover_sha256": null,
        "activities": [], "attachments": [], "reminders": []
    })
}

fn export_shell(object: serde_json::Value) -> serde_json::Value {
    json!({ "version": 1, "exported_at": "2024-01-01T00:00:00Z", "currency": "EUR", "objects": [object] })
}

fn png() -> Vec<u8> {
    let img = image::DynamicImage::new_rgb8(64, 32);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[tokio::test]
async fn export_import_round_trip() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let act: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2024-01-01", "category": "repair", "title": "Brakes", "cost_cents": 12345, "counter_value": 100 }))
        .send().await.unwrap().json().await.unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));
    app.client.post(&base).multipart(Form::new().part("file", Part::bytes(png()).file_name("a.png").mime_str("image/png").unwrap()).text("activity_id", act["id"].to_string())).send().await.unwrap();
    app.client.post(&base).multipart(Form::new().part("file", Part::bytes(b"manual".to_vec()).file_name("m.txt").mime_str("text/plain").unwrap())).send().await.unwrap();
    app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({ "title": "Oil", "due_counter": 5000, "repeat_counter": 5000 })).send().await.unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({ "title": "Done one", "due_date": "2020-01-01" })).send().await.unwrap().json().await.unwrap();
    app.client.post(app.url(&format!("/reminders/{}/done", r["id"]))).json(&json!({ "activity_id": act["id"] })).send().await.unwrap();

    let res = app.client.get(app.url("/export")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "application/zip");
    let zip_bytes = res.bytes().await.unwrap().to_vec();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes.clone())).unwrap();
    assert!(z.by_name("data.json").is_ok());
    assert_eq!(z.len(), 3, "data.json + 2 blobs");

    // import into a different user
    let anna = app.create_user_client("anna", "password123").await;
    let res = anna.post(app.url("/import")).header("content-type", "application/zip").body(zip_bytes).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let counts: serde_json::Value = res.json().await.unwrap();
    assert_eq!(counts["objects"], 1);
    assert_eq!(counts["activities"], 1);
    assert_eq!(counts["attachments"], 2);
    assert_eq!(counts["reminders"], 2);

    let objs: Vec<serde_json::Value> = anna.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objs[0]["name"], "Golf");
    assert_eq!(objs[0]["stats"]["total_cost_cents"], 12345);
    let nid = objs[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = anna.get(app.url(&format!("/objects/{nid}/activities"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(acts[0]["attachments"][0]["original_name"], "a.png");
    let thumb = anna.get(app.url(&format!("/files/{}/thumb", acts[0]["attachments"][0]["file_id"]))).send().await.unwrap();
    assert_eq!(thumb.status(), 200);
    let rems: Vec<serde_json::Value> = anna.get(app.url(&format!("/objects/{nid}/reminders"))).send().await.unwrap().json().await.unwrap();
    let done = rems.iter().find(|r| r["title"] == "Done one").unwrap();
    assert_eq!(done["done_activity_id"], acts[0]["id"]);

    // single-object export
    let res = app.client.get(app.url(&format!("/export?object_id={id}"))).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let res = app.client.get(app.url("/export?object_id=9999")).send().await.unwrap();
    assert_eq!(res.status(), 404);
    let res = anna.post(app.url("/import")).header("content-type", "application/zip").body(b"nope".to_vec()).send().await.unwrap();
    assert_eq!(res.status(), 400);
}

/// A well-formed version-1 archive whose reminder carries an unparseable `due_date`
/// must be rejected outright, and must not leave a partial import behind.
#[tokio::test]
async fn import_rejects_invalid_reminder_due_date() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;

    let mut object = base_object();
    object["reminders"] = json!([{
        "title": "Oil", "notes": "", "due_date": "not-a-date", "due_counter": null,
        "repeat_months": null, "repeat_counter": null, "done_at": null,
        "done_activity_index": null, "created_at": "2024-01-01T00:00:00Z"
    }]);
    let zip_bytes = zip_data_json(&export_shell(object));

    let res = anna.post(app.url("/import")).header("content-type", "application/zip").body(zip_bytes).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objs.len(), 0, "rejected import must not persist anything");

    // The reminders read path must survive: no panic, a clean 200.
    let res = anna.get(app.url("/reminders/due")).send().await.unwrap();
    assert_eq!(res.status(), 200);
}

/// A well-formed version-1 archive whose activity carries a `category` outside the
/// fixed set must also be rejected, and must not leave a partial import behind.
#[tokio::test]
async fn import_rejects_invalid_activity_category() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;

    let mut object = base_object();
    object["activities"] = json!([{
        "date": "2024-01-01", "category": "not-a-category", "title": "Oops", "notes": "",
        "counter_value": null, "cost_cents": null, "created_at": "2024-01-01T00:00:00Z", "attachments": []
    }]);
    let zip_bytes = zip_data_json(&export_shell(object));

    let res = anna.post(app.url("/import")).header("content-type", "application/zip").body(zip_bytes).send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objs.len(), 0, "rejected import must not persist anything");
}
