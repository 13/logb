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
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({ "name": "Golf", "type": "car", "counter_unit": "km", "cover_attachment_id": a1["id"] })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let obj: serde_json::Value = res.json().await.unwrap();
    assert_eq!(obj["cover_attachment_id"], a1["id"]);
    assert_eq!(obj["cover_file_id"], a1["file_id"]);

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

    // A single activity -- read, or answered after a save -- carries its own attachment and not
    // the object's document.
    for res in [
        app.client.get(app.url(&format!("/activities/{}", act["id"]))).send().await.unwrap(),
        app.client.patch(app.url(&format!("/activities/{}", act["id"])))
            .json(&json!({ "date": "2024-01-01", "category": "repair", "title": "Brake pads" }))
            .send().await.unwrap(),
    ] {
        let status = res.status();
        let body = res.text().await.unwrap();
        assert_eq!(status, 200, "{body}");
        let one: serde_json::Value = serde_json::from_str(&body).unwrap();
        let names: Vec<&str> = one["attachments"].as_array().unwrap().iter()
            .map(|a| a["original_name"].as_str().unwrap()).collect();
        assert_eq!(names, ["invoice.pdf"]);
    }

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
        .json(&json!({ "name": "Golf", "type": "car", "counter_unit": "km", "cover_attachment_id": photo["id"] }))
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

/// A cover must be a photo of the object being patched: pointing at another object's photo
/// is a 400, not a silent cross-object reference.
#[tokio::test]
async fn cover_must_belong_to_the_patched_object() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let a = app.create_object(&app.client, "Golf", None).await;
    let b = app.create_object(&app.client, "Bike", None).await;
    let (a_id, b_id) = (a["id"].as_i64().unwrap(), b["id"].as_i64().unwrap());

    let photo: serde_json::Value = app.client
        .post(app.url(&format!("/objects/{a_id}/attachments")))
        .multipart(form(png(40, 40), "front.png", "image/png"))
        .send().await.unwrap().json().await.unwrap();

    let res = app.client.patch(app.url(&format!("/objects/{b_id}")))
        .json(&json!({ "name": "Bike", "type": "car", "cover_attachment_id": photo["id"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 400, "another object's photo must not become this object's cover");

    // A document of the right object is refused too -- covers are photos.
    let doc: serde_json::Value = app.client
        .post(app.url(&format!("/objects/{b_id}/attachments")))
        .multipart(form(b"manual".to_vec(), "m.txt", "text/plain"))
        .send().await.unwrap().json().await.unwrap();
    let res = app.client.patch(app.url(&format!("/objects/{b_id}")))
        .json(&json!({ "name": "Bike", "type": "car", "cover_attachment_id": doc["id"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 400);
}

/// Sending `cover_attachment_id: null` clears the cover; omitting the field keeps it.
#[tokio::test]
async fn cover_can_be_set_kept_and_cleared() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    let photo: serde_json::Value = app.client
        .post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(form(png(40, 40), "front.png", "image/png"))
        .send().await.unwrap().json().await.unwrap();

    let url = app.url(&format!("/objects/{id}"));
    let set: serde_json::Value = app.client.patch(&url)
        .json(&json!({ "name": "Golf", "type": "car", "cover_attachment_id": photo["id"] }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(set["cover_attachment_id"], photo["id"]);
    assert_eq!(set["cover_file_id"], photo["file_id"]);

    // Field omitted: the cover survives an unrelated edit.
    let kept: serde_json::Value = app.client.patch(&url)
        .json(&json!({ "name": "Golf GTI", "type": "car" }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(kept["cover_attachment_id"], photo["id"]);

    // Explicit null: cleared.
    let cleared: serde_json::Value = app.client.patch(&url)
        .json(&json!({ "name": "Golf GTI", "type": "car", "cover_attachment_id": null }))
        .send().await.unwrap().json().await.unwrap();
    assert!(cleared["cover_attachment_id"].is_null(), "{cleared}");
    assert!(cleared["cover_file_id"].is_null(), "{cleared}");
}

/// Deleting an object must take its files with it, not just its attachment rows.
#[tokio::test]
async fn deleting_an_object_makes_its_files_unreadable() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    let photo: serde_json::Value = app.client
        .post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(form(png(40, 40), "front.png", "image/png"))
        .send().await.unwrap().json().await.unwrap();
    let fid = photo["file_id"].as_i64().unwrap();
    assert_eq!(app.client.get(app.url(&format!("/files/{fid}"))).send().await.unwrap().status(), 200);
    assert_eq!(app.client.get(app.url(&format!("/files/{fid}/thumb"))).send().await.unwrap().status(), 200);

    assert_eq!(app.client.delete(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 204);
    // Nothing is purged here: the row and blob deliberately survive the sync window so an
    // offline client can still be told what it lost. `load_owned_file` is what makes both
    // routes read as 404 in the meantime.
    assert_eq!(app.client.get(app.url(&format!("/files/{fid}"))).send().await.unwrap().status(), 404);
    assert_eq!(app.client.get(app.url(&format!("/files/{fid}/thumb"))).send().await.unwrap().status(), 404);
}

/// A non-ASCII filename has to survive as `filename*`, and must not knock the response back
/// to a bare `inline`.
#[tokio::test]
async fn download_keeps_a_non_ascii_filename() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    let doc: serde_json::Value = app.client
        .post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(form(b"rechnung".to_vec(), "Anhängerkupplung — Rechnung.txt", "text/plain"))
        .send().await.unwrap().json().await.unwrap();
    let fid = doc["file_id"].as_i64().unwrap();

    let res = app.client.get(app.url(&format!("/files/{fid}"))).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let cd = res.headers()["content-disposition"].to_str().unwrap().to_string();
    assert!(cd.starts_with("attachment;"), "{cd}");
    assert!(cd.contains("filename*=UTF-8''Anh%C3%A4ngerkupplung"), "{cd}");
}

/// An SVG is an image by MIME type and a scriptable document in practice. Serving one inline
/// from this origin would let it run against the uploader's own session, so it downloads --
/// and every file response carries nosniff and a sandbox policy.
#[tokio::test]
async fn an_uploaded_svg_cannot_run_in_the_apps_origin() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#.to_vec();
    let att: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(form(svg, "x.svg", "image/svg+xml"))
        .send().await.unwrap().json().await.unwrap();
    let fid = att["file_id"].as_i64().unwrap();

    let res = app.client.get(app.url(&format!("/files/{fid}"))).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let h = res.headers();
    let disposition = h["content-disposition"].to_str().unwrap();
    assert!(disposition.starts_with("attachment;"), "an SVG must download, not render: {disposition}");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(h["content-security-policy"].to_str().unwrap().contains("sandbox"), "{:?}", h["content-security-policy"]);
}

/// Photos still render in place -- the allow-list must not have broken the common case.
#[tokio::test]
async fn photos_and_pdfs_still_render_inline() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    let att: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(form(png(40, 40), "front.png", "image/png"))
        .send().await.unwrap().json().await.unwrap();
    let res = app.client.get(app.url(&format!("/files/{}", att["file_id"]))).send().await.unwrap();
    assert!(res.headers()["content-disposition"].to_str().unwrap().starts_with("inline;"));
}

#[tokio::test]
async fn a_replayed_upload_returns_the_first_attachment() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));

    let send = || async {
        let part = Part::bytes(png(10, 10)).file_name("a.png").mime_str("image/png").unwrap();
        let form = Form::new().part("file", part).text("client_op_id", "up-abc-123");
        app.client.post(app.url(&format!("/objects/{id}/attachments")))
            .multipart(form).send().await.unwrap()
    };

    let first = send().await;
    assert_eq!(first.status(), 201);
    let first: serde_json::Value = first.json().await.unwrap();
    let again = send().await;
    assert_eq!(again.status(), 200);
    let again: serde_json::Value = again.json().await.unwrap();
    assert_eq!(again["id"], first["id"]);

    let list: Vec<serde_json::Value> = app.client.get(&base).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1, "a replayed upload must leave exactly one attachment row on the object");
}

/// The upload equivalent of `many_activities_with_no_client_op_id_do_not_conflict`: the
/// partial unique index only guards non-null values, so every ordinary (non-outbox) upload,
/// which sends no client_op_id at all, must coexist freely.
#[tokio::test]
async fn many_uploads_with_no_client_op_id_do_not_conflict() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));

    for i in 0..5 {
        let part = Part::bytes(png(10, 10)).file_name(format!("a{i}.png")).mime_str("image/png").unwrap();
        let res = app.client.post(&base).multipart(Form::new().part("file", part)).send().await.unwrap();
        assert_eq!(res.status(), 201, "an absent client_op_id must never collide");
    }

    let list: Vec<serde_json::Value> = app.client.get(&base).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 5);
}

/// The upload equivalent of `one_client_op_id_cannot_be_reused_across_objects`.
#[tokio::test]
async fn one_client_op_id_cannot_be_reused_across_objects_for_uploads() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let a = app.create_object(&app.client, "Golf", Some("km")).await;
    let b = app.create_object(&app.client, "Bike", Some("km")).await;
    let (aid, bid) = (a["id"].as_i64().unwrap(), b["id"].as_i64().unwrap());

    let upload = |object_id: i64| {
        let part = Part::bytes(png(10, 10)).file_name("a.png").mime_str("image/png").unwrap();
        let form = Form::new().part("file", part).text("client_op_id", "up-dup");
        app.client.post(app.url(&format!("/objects/{object_id}/attachments"))).multipart(form).send()
    };

    assert_eq!(upload(aid).await.unwrap().status(), 201);
    let res = upload(bid).await.unwrap();
    assert_eq!(res.status(), 409, "the id is the client's promise that this is the same op");
}

/// As `blank_client_op_id_is_treated_as_absent` for the JSON path: a blank multipart field
/// must not be treated as a real idempotency key either.
#[tokio::test]
async fn blank_client_op_id_is_treated_as_absent_for_uploads() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));

    for op_id in ["", "   "] {
        let part = Part::bytes(png(10, 10)).file_name("a.png").mime_str("image/png").unwrap();
        let form = Form::new().part("file", part).text("client_op_id", op_id);
        let res = app.client.post(&base).multipart(form).send().await.unwrap();
        assert_eq!(res.status(), 201, "a blank (or whitespace-only) client_op_id must not block a real upload");
    }

    let list: Vec<serde_json::Value> = app.client.get(&base).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 2, "two blank-id uploads must produce two distinct rows, not one");
}

/// Counts the `files` rows the instance holds, across every user.
async fn file_row_count(app: &common::TestApp) -> i64 {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM files")
        .fetch_one(&app.state.db).await.unwrap();
    n
}

/// The unique-violation recovery arms in `upload` were previously proven only by code trace and
/// by a raw INSERT against the index itself -- nothing drove two genuinely concurrent requests
/// through them. These two tests do, by firing a batch of uploads at the running server at once:
/// every one of them drains its body and runs its pre-check before any of them reaches the
/// INSERT, so the losers arrive at a row that was not there when they looked.
#[tokio::test]
async fn concurrent_uploads_of_identical_bytes_share_one_file_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));

    // A document, not an image: an image upload hops through `spawn_blocking` to build its
    // thumbnail, and that hop is long enough that the losers' `files` SELECT lands after the
    // winner's INSERT has already committed -- they take the dedup path and the race never
    // happens. With no image processing in the way, the SELECT-then-INSERT window is the whole
    // window, and the losers do collide.
    let bytes = b"%PDF-1.4 concurrent".to_vec();
    let mut tasks = Vec::new();
    for i in 0..8 {
        let (client, base, bytes) = (app.client.clone(), base.clone(), bytes.clone());
        tasks.push(tokio::spawn(async move {
            client.post(&base).multipart(form(bytes, &format!("manual{i}.pdf"), "application/pdf")).send().await.unwrap()
        }));
    }

    let mut file_ids = Vec::new();
    let mut attachment_ids = Vec::new();
    for t in tasks {
        let res = t.await.unwrap();
        assert_eq!(res.status(), 201);
        let a: serde_json::Value = res.json().await.unwrap();
        file_ids.push(a["file_id"].as_i64().unwrap());
        attachment_ids.push(a["id"].as_i64().unwrap());
    }
    // Identical bytes, so every attachment must point at the one file row the winner created --
    // the losers' INSERTs trip UNIQUE(user_id, sha256) and adopt it.
    assert!(file_ids.windows(2).all(|w| w[0] == w[1]), "one file row expected, got {file_ids:?}");
    attachment_ids.sort_unstable();
    attachment_ids.dedup();
    assert_eq!(attachment_ids.len(), 8, "each upload is its own attachment");
    assert_eq!(file_row_count(&app).await, 1);
    assert_eq!(app.client.get(app.url(&format!("/files/{}", file_ids[0]))).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn concurrent_uploads_sharing_one_op_id_resolve_to_one_attachment_and_leave_no_orphan() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));

    // DIFFERENT bytes per request, which is the contract-violating case: each upload hashes to
    // its own sha, so a loser has already written its own blob and `files` row by the time its
    // attachment INSERT trips the op-id index. Whatever it wrote is referenced by nothing.
    let mut tasks = Vec::new();
    for i in 0..6 {
        let (client, base) = (app.client.clone(), base.clone());
        let bytes = png(800 + i, 600);
        tasks.push(tokio::spawn(async move {
            client.post(&base)
                .multipart(form(bytes, &format!("shot{i}.png"), "image/png").text("client_op_id", "op-shared"))
                .send().await.unwrap()
        }));
    }

    let mut attachment_ids = Vec::new();
    for t in tasks {
        let res = t.await.unwrap();
        assert!(res.status() == 200 || res.status() == 201, "{}", res.status());
        let a: serde_json::Value = res.json().await.unwrap();
        attachment_ids.push(a["id"].as_i64().unwrap());
    }
    assert!(attachment_ids.windows(2).all(|w| w[0] == w[1]), "one op id, one attachment: {attachment_ids:?}");

    let list: serde_json::Value = app.client.get(&base).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    // The winner's file row is the only one that may survive: every loser's is unreferenced.
    assert_eq!(file_row_count(&app).await, 1, "a losing upload left its file row behind");
}

#[tokio::test]
async fn an_upload_honours_and_replays_on_client_uuid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let base = app.url(&format!("/objects/{}/attachments", car["id"]));
    let make = || form(png(64, 64), "a.png", "image/png").text("client_uuid", "phone-0004-photo");
    let res = app.client.post(&base).multipart(make()).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let first: serde_json::Value = res.json().await.unwrap();
    let res = app.client.post(&base).multipart(make()).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let second: serde_json::Value = res.json().await.unwrap();
    assert_eq!(first["id"], second["id"]);
    let list: Vec<serde_json::Value> = app.client.get(&base).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1);
}

#[tokio::test]
async fn an_attachment_response_names_its_own_uuid_and_its_files_uuid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let base = app.url(&format!("/objects/{}/attachments", car["id"]));
    let a: serde_json::Value = app.client.post(&base)
        .multipart(form(png(64, 64), "a.png", "image/png").text("client_uuid", "phone-0004-photo"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(a["client_uuid"], "phone-0004-photo");
    let file_uuid = a["file_uuid"].as_str().unwrap().to_string();
    // The same bytes again, as a second attachment: dedup means the same file uuid.
    let b: serde_json::Value = app.client.post(&base)
        .multipart(form(png(64, 64), "b.png", "image/png").text("client_uuid", "phone-0005-photo"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(b["file_uuid"], file_uuid);
    let boot: serde_json::Value = app.client.get(app.url("/sync/bootstrap")).send().await.unwrap().json().await.unwrap();
    assert!(boot["files"].as_array().unwrap().iter().any(|f| f["client_uuid"] == file_uuid));
}

/// File ids are sequential, so a response the browser may reuse without asking would give the
/// next person signed in on that browser the previous person's bytes for `/files/N` without the
/// ownership check ever running. Every reuse is revalidated instead, and the 304 that makes it
/// cheap is answered only to the owner.
#[tokio::test]
async fn file_responses_are_revalidated_and_a_304_needs_ownership() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let a: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(form(png(800, 600), "front.png", "image/png")).send().await.unwrap().json().await.unwrap();
    let fid = a["file_id"].as_i64().unwrap();

    let mut etags = Vec::new();
    for path in [format!("/files/{fid}"), format!("/files/{fid}/thumb")] {
        let url = app.url(&path);
        let res = app.client.get(&url).send().await.unwrap();
        assert_eq!(res.status(), 200, "{path}");
        assert_eq!(res.headers()["cache-control"], "private, no-cache", "{path}");
        let etag = res.headers()["etag"].to_str().unwrap().to_string();
        assert!(etag.starts_with('"') && etag.ends_with('"') && etag.len() > 2, "a strong ETag for {path}: {etag}");

        // The owner revalidating: 304, no body, same validator.
        let res = app.client.get(&url).header("If-None-Match", &etag).send().await.unwrap();
        assert_eq!(res.status(), 304, "{path}");
        assert_eq!(res.headers()["etag"], etag.as_str(), "{path}");
        assert_eq!(res.headers()["cache-control"], "private, no-cache", "{path}");
        assert!(res.bytes().await.unwrap().is_empty(), "{path}");

        // Someone else sending the very same validator: the ownership check answers, not the cache.
        let res = anna.get(&url).header("If-None-Match", &etag).send().await.unwrap();
        assert_eq!(res.status(), 404, "{path}");
        assert!(res.headers().get("etag").is_none(), "{path}");

        // A different validator, or none: the bytes.
        let res = app.client.get(&url).header("If-None-Match", "\"something-else\"").send().await.unwrap();
        assert_eq!(res.status(), 200, "{path}");
        assert!(!res.bytes().await.unwrap().is_empty(), "{path}");
        let res = app.client.get(&url).send().await.unwrap();
        assert_eq!(res.status(), 200, "{path}");
        etags.push(etag);
    }
    assert_ne!(etags[0], etags[1], "the thumbnail is different bytes, so a different validator");
}

/// SQLite hands a rolled-back `AUTOINCREMENT` id straight to the next insert: the counter lives
/// in `sqlite_sequence`, an ordinary table the rollback restores. A thumbnail named after the
/// file id and written inside the failed transaction therefore used to outlive it and be served
/// as the thumbnail of whatever unrelated file got that id next -- here a PDF, which has no
/// thumbnail at all.
#[tokio::test]
async fn a_rolled_back_upload_leaves_no_thumbnail_a_later_file_serves() {
    if common::skipped_on_postgres(
        "a_rolled_back_upload_leaves_no_thumbnail_a_later_file_serves",
        "PostgreSQL sequences never hand a rolled-back id out again, and the trigger is SQLite's",
    ) {
        return;
    }
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let base = app.url(&format!("/objects/{}/attachments", car["id"]));

    app.fail_changes_for("file").await;
    let res = app.client.post(&base).multipart(form(png(800, 600), "front.png", "image/png")).send().await.unwrap();
    assert_eq!(res.status(), 500, "{}", res.text().await.unwrap());
    app.stop_failing_changes_for("file").await;
    assert_eq!(file_row_count(&app).await, 0, "the failed upload committed its row");

    let res = app.client.post(&base).multipart(form(b"%PDF-1.4 manual".to_vec(), "manual.pdf", "application/pdf")).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let pdf: serde_json::Value = res.json().await.unwrap();
    assert_eq!(pdf["file_id"], 1, "the precondition: the rolled-back id was handed out again");

    let thumb = app.client.get(app.url(&format!("/files/{}/thumb", pdf["file_id"]))).send().await.unwrap();
    assert_eq!(thumb.status(), 404, "a PDF was served the failed upload's thumbnail");
}

/// An instance upgraded from the id-named layout keeps every thumbnail a live image row owns,
/// loses the ones nothing could rightly serve, and running the move again changes nothing.
#[tokio::test]
async fn id_named_thumbnails_move_to_their_content_name_once() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let base = app.url(&format!("/objects/{}/attachments", car["id"]));
    let photo: serde_json::Value = app.client.post(&base).multipart(form(png(800, 600), "front.png", "image/png"))
        .send().await.unwrap().json().await.unwrap();
    let pdf: serde_json::Value = app.client.post(&base).multipart(form(b"%PDF-1.4 m".to_vec(), "m.pdf", "application/pdf"))
        .send().await.unwrap().json().await.unwrap();
    let (sha,): (String,) = sqlx::query_as("SELECT sha256 FROM files WHERE id = $1")
        .bind(photo["file_id"].as_i64().unwrap()).fetch_one(&app.state.db).await.unwrap();

    // Put the disk back the way the old layout had it: the photo's thumbnail under its id, a
    // stray under the document's id (a rolled-back upload's), and one under an id nobody has.
    let storage = &app.state.storage;
    let jpeg = std::fs::read(storage.thumb_path(&sha)).unwrap();
    std::fs::remove_file(storage.thumb_path(&sha)).unwrap();
    let legacy = |id: &serde_json::Value| storage.thumbs_dir().join(format!("{id}.jpg"));
    std::fs::write(legacy(&photo["file_id"]), &jpeg).unwrap();
    std::fs::write(legacy(&pdf["file_id"]), b"stray").unwrap();
    std::fs::write(legacy(&json!(9999)), b"stray").unwrap();

    let report = logb::files_gc::migrate_legacy_thumbs(&app.state).await.unwrap();
    assert_eq!(report, logb::files_gc::LegacyThumbs { moved: 1, deleted: 2 });
    assert_eq!(std::fs::read(storage.thumb_path(&sha)).unwrap(), jpeg);
    for id in [&photo["file_id"], &pdf["file_id"], &json!(9999)] {
        assert!(!legacy(id).exists(), "{id}.jpg is still there");
    }
    let thumb = app.client.get(app.url(&format!("/files/{}/thumb", photo["file_id"]))).send().await.unwrap();
    assert_eq!(thumb.status(), 200);
    let thumb = app.client.get(app.url(&format!("/files/{}/thumb", pdf["file_id"]))).send().await.unwrap();
    assert_eq!(thumb.status(), 404);

    let again = logb::files_gc::migrate_legacy_thumbs(&app.state).await.unwrap();
    assert_eq!(again, logb::files_gc::LegacyThumbs::default());
    assert_eq!(std::fs::read(storage.thumb_path(&sha)).unwrap(), jpeg);
}

/// Runs `work` while a write transaction is held open, and says whether it finished before that
/// transaction ended -- i.e. whether it ran without the write lock.
async fn finishes_while_a_writer_holds_the_lock<F>(app: &common::TestApp, work: F) -> bool
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let tx = logb::db::begin_write(&app.state).await.unwrap();
    let task = tokio::spawn(work);
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let finished_early = task.is_finished();
    tx.rollback().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), task).await
        .expect("still waiting after the writer let go").unwrap();
    finished_early
}

/// Discarding a blob is a check ("no row names this hash") followed by an unlink. Run outside
/// the write lock, an upload of the same bytes could commit its row between the two and be left
/// pointing at nothing. Both halves now happen under the write connection, so a writer holding
/// it keeps the discard waiting.
#[tokio::test]
async fn discarding_a_blob_waits_for_the_write_lock() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bytes = b"%PDF-1.4 nobody's".to_vec();
    let sha = logb::files::sha256_hex(&bytes);
    app.state.storage.write_blob(&sha, &bytes).await.unwrap();
    let blob = app.state.storage.blob_path(&sha);

    let (state, s) = (app.state.clone(), sha.clone());
    let early = finishes_while_a_writer_holds_the_lock(&app, async move {
        logb::api::attachments::discard_blob(&state, &s).await.unwrap();
    }).await;
    assert!(!early, "the discard ran without the write lock");
    assert!(!blob.exists(), "an unreferenced blob survived the discard");
}

/// The same for the orphan purge: its "no attachment references this file" check and the
/// delete belong to one writer's turn.
#[tokio::test]
async fn purging_orphan_files_waits_for_the_write_lock() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (user_id,): (i64,) = sqlx::query_as("SELECT id FROM users LIMIT 1").fetch_one(&app.state.db).await.unwrap();
    let (file_id,): (i64,) = sqlx::query_as(
        "INSERT INTO files (user_id, sha256, original_name, mime, size, created_at, client_uuid) \
         VALUES ($1, $2, 'o.pdf', 'application/pdf', 1, '2024-01-01T00:00:00Z', 'orphan-file-uuid') RETURNING id")
        .bind(user_id).bind("ab".repeat(32)).fetch_one(&app.state.write_db).await.unwrap();

    let state = app.state.clone();
    let early = finishes_while_a_writer_holds_the_lock(&app, async move {
        logb::api::attachments::purge_orphan_files(&state, &[file_id]).await.unwrap();
    }).await;
    assert!(!early, "the purge ran without the write lock");
    assert_eq!(file_row_count(&app).await, 0, "the orphan row survived the purge");
}

/// Backdates a file's modification time by `hours`.
fn age(path: &std::path::Path, hours: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(hours * 3600);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
}

/// Plants `bytes` at `path`, creating its shard directory, aged by `hours`.
fn plant(path: &std::path::Path, bytes: &[u8], hours: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    age(path, hours);
}

/// Whatever a failed request, a crash or a lost race leaves on disk is collected once two sweeps
/// a day apart have found nothing naming it: blobs and thumbnails no `files` row names. Stranded
/// `.part`/`.tmp` scratch goes as soon as it is a day old. Anything younger is left alone -- it
/// may belong to a request still in flight -- and anything a row names is kept however old it is.
#[tokio::test]
async fn the_sweep_collects_old_orphans_and_nothing_else() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let photo: serde_json::Value = app.client.post(app.url(&format!("/objects/{}/attachments", car["id"])))
        .multipart(form(png(800, 600), "front.png", "image/png")).send().await.unwrap().json().await.unwrap();
    let (kept,): (String,) = sqlx::query_as("SELECT sha256 FROM files WHERE id = $1")
        .bind(photo["file_id"].as_i64().unwrap()).fetch_one(&app.state.db).await.unwrap();
    let storage = &app.state.storage;
    age(&storage.blob_path(&kept), 48);
    age(&storage.thumb_path(&kept), 48);

    let old = "0a".repeat(32);
    let young = "0b".repeat(32);
    plant(&storage.blob_path(&old), b"old orphan", 25);
    plant(&storage.thumb_path(&old), b"old orphan", 25);
    plant(&storage.blob_path(&young), b"young orphan", 1);
    plant(&storage.thumb_path(&young), b"young orphan", 1);
    let old_part = storage.blob_path(&old).with_extension("deadbeef.part");
    let old_thumb_part = storage.thumb_path(&old).with_extension("deadbeef.part");
    let old_tmp = storage.files_dir().join(".export-deadbeef.tmp");
    let young_tmp = storage.files_dir().join(".import-cafebabe.tmp");
    plant(&old_part, b"torn", 25);
    plant(&old_thumb_part, b"torn", 25);
    plant(&old_tmp, b"stranded", 25);
    plant(&young_tmp, b"in flight", 1);

    let grace = logb::files_gc::GRACE;
    let now = std::time::SystemTime::now();
    let mut sweeper = logb::files_gc::Sweeper::new();

    // The first sweep takes the old scratch and only notes the old orphans.
    let first = sweeper.sweep_at(&app.state, grace, now).await.unwrap();
    assert_eq!(first, logb::files_gc::Swept { blobs: 0, thumbs: 0, scratch: 3 });
    for gone in [&old_part, &old_thumb_part, &old_tmp] {
        assert!(!gone.exists(), "{} survived the sweep", gone.display());
    }
    for kept in [storage.blob_path(&old), storage.thumb_path(&old), young_tmp.clone()] {
        assert!(kept.exists(), "{} went on its first sighting", kept.display());
    }

    // A day later the old orphans go. The young ones are now old enough, but this is their
    // first sighting; the young scratch is simply a day older.
    let later = now + grace + std::time::Duration::from_secs(3600);
    let second = sweeper.sweep_at(&app.state, grace, later).await.unwrap();
    assert_eq!(second, logb::files_gc::Swept { blobs: 1, thumbs: 1, scratch: 1 });
    for gone in [storage.blob_path(&old), storage.thumb_path(&old), young_tmp] {
        assert!(!gone.exists(), "{} survived the sweep", gone.display());
    }
    for kept in [storage.blob_path(&kept), storage.thumb_path(&kept), storage.blob_path(&young), storage.thumb_path(&young)] {
        assert!(kept.exists(), "{} was swept", kept.display());
    }
    let thumb = app.client.get(app.url(&format!("/files/{}/thumb", photo["file_id"]))).send().await.unwrap();
    assert_eq!(thumb.status(), 200);
}

/// A data directory full of files no row names is a database that does not belong to it -- an
/// empty one, the wrong URL, an older snapshot restored -- not a pile of failed uploads. The
/// sweep deletes none of them, however many days it runs.
#[tokio::test]
async fn the_sweep_stands_down_when_the_database_does_not_match_the_files() {
    let app = common::spawn().await;
    let storage = &app.state.storage;
    let shas: Vec<String> = (0..logb::files_gc::MISMATCH_FLOOR).map(|i| format!("{:064x}", i + 1)).collect();
    for sha in &shas {
        plant(&storage.blob_path(sha), b"someone's attachment", 72);
        plant(&storage.thumb_path(sha), b"its thumbnail", 72);
    }

    let grace = logb::files_gc::GRACE;
    let now = std::time::SystemTime::now();
    let mut sweeper = logb::files_gc::Sweeper::new();
    for day in 0..3u64 {
        let at = now + std::time::Duration::from_secs(day * 25 * 3600);
        let swept = sweeper.sweep_at(&app.state, grace, at).await.unwrap();
        assert_eq!(swept, logb::files_gc::Swept::default(), "day {day}");
    }
    for sha in &shas {
        assert!(storage.blob_path(sha).exists() && storage.thumb_path(sha).exists());
    }
}

/// The one-time thumbnail move trusts the `files` table to say which id-named thumbnails are
/// strays. Against a table with no rows at all it trusts nothing and deletes nothing.
#[tokio::test]
async fn the_thumbnail_move_leaves_everything_when_there_are_no_file_rows() {
    let app = common::spawn().await;
    let legacy = app.state.storage.thumbs_dir().join("7.jpg");
    std::fs::write(&legacy, b"a thumbnail from before").unwrap();
    let report = logb::files_gc::migrate_legacy_thumbs(&app.state).await.unwrap();
    assert_eq!(report, logb::files_gc::LegacyThumbs::default());
    assert!(legacy.exists());
}

/// The original is streamed from disk rather than read into memory first; what arrives must
/// still be every byte, with a length the client can show progress against.
#[tokio::test]
async fn an_original_streams_back_whole_with_its_length() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let bytes = png(1200, 900);
    let a: serde_json::Value = app.client.post(app.url(&format!("/objects/{}/attachments", car["id"])))
        .multipart(form(bytes.clone(), "big.png", "image/png")).send().await.unwrap().json().await.unwrap();
    let res = app.client.get(app.url(&format!("/files/{}", a["file_id"]))).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-length"], bytes.len().to_string().as_str());
    assert_eq!(res.bytes().await.unwrap().to_vec(), bytes);
}
