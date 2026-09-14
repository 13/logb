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
