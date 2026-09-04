mod common;
use reqwest::multipart::{Form, Part};
use serde_json::json;

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::DynamicImage::new_rgb8(w, h);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn form(bytes: Vec<u8>, name: &str, mime: &str) -> Form {
    Form::new().part("file", Part::bytes(bytes).file_name(name.to_string()).mime_str(mime).unwrap())
}

#[tokio::test]
async fn upload_photo_dedup_thumb_and_serve() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));

    let res = app.client.post(&base).multipart(form(png(800, 600), "front.png", "image/png")).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let a1: serde_json::Value = res.json().await.unwrap();
    assert_eq!(a1["kind"], "photo");
    assert_eq!(a1["width"], 800);
    assert_eq!(a1["height"], 600);
    assert_eq!(a1["original_name"], "front.png");
    let fid = a1["file_id"].as_i64().unwrap();

    // same bytes again -> same file row, new attachment
    let res = app.client.post(&base).multipart(form(png(800, 600), "copy.png", "image/png").text("caption", "again")).send().await.unwrap();
    let a2: serde_json::Value = res.json().await.unwrap();
    assert_eq!(a2["file_id"], fid);
    assert_ne!(a2["id"], a1["id"]);
    assert_eq!(a2["caption"], "again");

    let orig = app.client.get(app.url(&format!("/files/{fid}"))).send().await.unwrap();
    assert_eq!(orig.status(), 200);
    assert_eq!(orig.headers()["content-type"], "image/png");
    let thumb = app.client.get(app.url(&format!("/files/{fid}/thumb"))).send().await.unwrap();
    assert_eq!(thumb.status(), 200);
    assert_eq!(thumb.headers()["content-type"], "image/jpeg");
    let t = image::load_from_memory(&thumb.bytes().await.unwrap()).unwrap();
    assert_eq!((t.width(), t.height()), (400, 300));

    // cover
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({ "name": "Golf", "category": "car", "counter_unit": "km", "cover_attachment_id": a1["id"] })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let obj: serde_json::Value = res.json().await.unwrap();
    assert_eq!(obj["cover_attachment_id"], a1["id"]);

    // delete one attachment: file stays (still referenced); delete the other: file + blobs gone
    assert_eq!(app.client.delete(app.url(&format!("/attachments/{}", a2["id"]))).send().await.unwrap().status(), 204);
    assert_eq!(app.client.get(app.url(&format!("/files/{fid}"))).send().await.unwrap().status(), 200);
    assert_eq!(app.client.delete(app.url(&format!("/attachments/{}", a1["id"]))).send().await.unwrap().status(), 204);
    assert_eq!(app.client.get(app.url(&format!("/files/{fid}"))).send().await.unwrap().status(), 404);
    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert!(obj["cover_attachment_id"].is_null(), "cover cleared");
}

#[tokio::test]
async fn documents_and_activity_attachments() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let act: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2024-01-01", "category": "repair", "title": "Brakes" })).send().await.unwrap().json().await.unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));

    let res = app.client.post(&base).multipart(form(b"%PDF-1.4 fake".to_vec(), "invoice.pdf", "application/pdf").text("activity_id", act["id"].to_string())).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let inv: serde_json::Value = res.json().await.unwrap();
    assert_eq!(inv["kind"], "document");
    assert_eq!(inv["activity_id"], act["id"]);
    assert!(inv["width"].is_null());

    let res = app.client.post(&base).multipart(form(b"manual".to_vec(), "manual.txt", "text/plain")).send().await.unwrap();
    assert_eq!(res.status(), 201);

    let all: Vec<serde_json::Value> = app.client.get(&base).send().await.unwrap().json().await.unwrap();
    assert_eq!(all.len(), 2, "object documents tab lists everything");
    let only_act: Vec<serde_json::Value> = app.client.get(format!("{base}?activity_id={}", act["id"])).send().await.unwrap().json().await.unwrap();
    assert_eq!(only_act.len(), 1);

    let acts: Vec<serde_json::Value> = app.client.get(app.url(&format!("/objects/{id}/activities"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(acts[0]["attachments"].as_array().unwrap().len(), 1);
    assert_eq!(acts[0]["attachments"][0]["original_name"], "invoice.pdf");

    let dl = app.client.get(app.url(&format!("/files/{}", inv["file_id"]))).send().await.unwrap();
    assert!(dl.headers()["content-disposition"].to_str().unwrap().contains("invoice.pdf"));
    assert_eq!(app.client.get(app.url(&format!("/files/{}/thumb", inv["file_id"]))).send().await.unwrap().status(), 404);

    // deleting the activity cascades its attachment and purges the file
    assert_eq!(app.client.delete(app.url(&format!("/activities/{}", act["id"]))).send().await.unwrap().status(), 204);
    assert_eq!(app.client.get(app.url(&format!("/files/{}", inv["file_id"]))).send().await.unwrap().status(), 404);
}

#[tokio::test]
async fn activity_delete_clears_stale_cover() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let act: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2024-01-01", "category": "repair", "title": "Brakes" })).send().await.unwrap().json().await.unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));

    let res = app.client.post(&base)
        .multipart(form(png(10, 10), "front.png", "image/png").text("activity_id", act["id"].to_string()))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let photo: serde_json::Value = res.json().await.unwrap();

    // set as cover
    let res = app.client.patch(app.url(&format!("/objects/{id}")))
        .json(&json!({ "name": "Golf", "category": "car", "counter_unit": "km", "cover_attachment_id": photo["id"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
    let obj: serde_json::Value = res.json().await.unwrap();
    assert_eq!(obj["cover_attachment_id"], photo["id"]);

    // deleting the activity cascades the attachment and must clear the now-dangling cover
    assert_eq!(app.client.delete(app.url(&format!("/activities/{}", act["id"]))).send().await.unwrap().status(), 204);
    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert!(obj["cover_attachment_id"].is_null(), "cover cleared after activity-cascade delete");
}

#[tokio::test]
async fn rejects_bad_uploads_and_isolates_users() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));

    let res = app.client.post(&base).multipart(form(b"MZ".to_vec(), "virus.exe", "application/octet-stream")).send().await.unwrap();
    assert_eq!(res.status(), 400);
    let res = app.client.post(&base).multipart(form(vec![0u8; 3 * 1024 * 1024], "big.bin", "image/png")).send().await.unwrap();
    assert_eq!(res.status(), 413, "test harness caps uploads at 2 MB");
    let res = app.client.post(&base).multipart(Form::new().text("caption", "no file")).send().await.unwrap();
    assert_eq!(res.status(), 400);
    let res = app.client.post(&base).multipart(form(png(10, 10), "a.png", "image/png").text("activity_id", "999")).send().await.unwrap();
    assert_eq!(res.status(), 404, "unknown activity");

    let a: serde_json::Value = app.client.post(&base).multipart(form(png(10, 10), "a.png", "image/png")).send().await.unwrap().json().await.unwrap();
    assert_eq!(anna.post(&base).multipart(form(png(10, 10), "a.png", "image/png")).send().await.unwrap().status(), 404);
    assert_eq!(anna.get(app.url(&format!("/files/{}", a["file_id"]))).send().await.unwrap().status(), 404);
    assert_eq!(anna.get(app.url(&format!("/files/{}/thumb", a["file_id"]))).send().await.unwrap().status(), 404);
    assert_eq!(anna.delete(app.url(&format!("/attachments/{}", a["id"]))).send().await.unwrap().status(), 404);
    assert_eq!(anna.patch(app.url(&format!("/attachments/{}", a["id"]))).json(&json!({ "caption": "x" })).send().await.unwrap().status(), 404);
}
