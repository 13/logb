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

/// Zips a version-1 archive holding one object: `fields` overlaid onto `base_object()`, the
/// same skeleton the tests below already mutate in place. A caller names only what is special
/// about the archive it wants -- a legacy `category`, say -- rather than every field
/// `ObjectExport` requires.
fn archive_with_object_json(fields: serde_json::Value) -> Vec<u8> {
    let mut object = base_object();
    let serde_json::Value::Object(map) = fields else { panic!("fields must be a JSON object") };
    for (k, v) in map {
        object[k] = v;
    }
    zip_data_json(&export_shell(object))
}

/// An archive written before object types exists on someone's disk. Importing it must apply the
/// same mapping the migration did -- otherwise every object in a year-old backup lands on
/// `other` and the restore quietly loses what kind of thing each one was.
#[tokio::test]
async fn an_archive_written_before_types_still_imports() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(json!({ "category": "Auto" }));

    let res = app.client.post(app.url("/import")).header("content-type", "application/zip").body(zip).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objs[0]["type"], "car");
}

/// Text that never mapped to a type is not discarded -- it is appended to the description on
/// its own line, exactly what the migration's `CASE` did for the rows already on disk.
#[tokio::test]
async fn unmapped_text_in_an_old_archive_reaches_the_description() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(json!({ "name": "Odd", "category": "Gravelbike Custom" }));

    let res = app.client.post(app.url("/import")).header("content-type", "application/zip").body(zip).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objs[0]["type"], "other");
    assert_eq!(objs[0]["description"], "Gravelbike Custom");
}

/// An archive that carries both fields at once (a hand edit, or a future export format that
/// grew a new field this app also still writes) trusts the valid `type` and ignores `category`
/// entirely -- `type` is what describes this app's current schema, `category` is a fallback for
/// when it is absent, not a second vote.
#[tokio::test]
async fn a_valid_type_wins_over_a_conflicting_category() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(json!({ "type": "bike", "category": "Auto" }));

    let res = app.client.post(app.url("/import")).header("content-type", "application/zip").body(zip).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objs[0]["type"], "bike");
}

/// A `type` this build does not recognise (a typo, or a future value) is not guessed at either
/// -- it falls back to `other`, same as an unmapped legacy word, but the description is left
/// alone: an illegal `type` string is not free text worth preserving.
#[tokio::test]
async fn an_illegal_type_falls_back_to_other_and_leaves_the_description_alone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(json!({ "type": "spaceship", "category": null, "description": "kept as is" }));

    let res = app.client.post(app.url("/import")).header("content-type", "application/zip").body(zip).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objs[0]["type"], "other");
    assert_eq!(objs[0]["description"], "kept as is");
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
    // The icon rides along so an archive is recognisable as LogB's at a glance. It is the
    // embedded SPA asset, not a second copy -- see `export::icon_bytes`.
    assert!(z.by_name("icon.svg").is_ok(), "the archive carries the logo");
    assert_eq!(z.len(), 4, "data.json + icon.svg + 2 blobs");

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

/// An archive with untrimmed whitespace around object name/category, activity title
/// and reminder title must be stored trimmed, matching what `POST /objects` et al.
/// already do -- import must not bypass the trimming every other write path applies.
#[tokio::test]
async fn import_trims_padded_strings() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;

    let mut object = base_object();
    object["name"] = json!("  Golf  ");
    object["category"] = json!("  car  ");
    object["activities"] = json!([{
        "date": "2024-01-01", "category": "repair", "title": "  Brakes  ", "notes": "",
        "counter_value": null, "cost_cents": null, "created_at": "2024-01-01T00:00:00Z", "attachments": []
    }]);
    object["reminders"] = json!([{
        "title": "  Oil  ", "notes": "", "due_date": null, "due_counter": 5000,
        "repeat_months": null, "repeat_counter": null, "done_at": null,
        "done_activity_index": null, "created_at": "2024-01-01T00:00:00Z"
    }]);
    let zip_bytes = zip_data_json(&export_shell(object));

    let res = anna.post(app.url("/import")).header("content-type", "application/zip").body(zip_bytes).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objs[0]["name"], "Golf");
    assert_eq!(objs[0]["type"], "car");
    let id = objs[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = anna.get(app.url(&format!("/objects/{id}/activities"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(acts[0]["title"], "Brakes");
    let rems: Vec<serde_json::Value> = anna.get(app.url(&format!("/objects/{id}/reminders"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(rems[0]["title"], "Oil");
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

/// A tiny archive whose entries inflate to far more than the import budget must be refused
/// before the bytes are buffered, not after. `max_import_mb` is 4 in the test harness, so the
/// decompression budget is 8 MiB; 32 MiB of zeroes deflates to a few kilobytes.
#[tokio::test]
async fn import_rejects_a_zip_bomb() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut cursor);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file("data.json", opts).unwrap();
        std::io::Write::write_all(&mut w, serde_json::to_vec(&export_shell(base_object())).unwrap().as_slice()).unwrap();
        w.start_file("files/0000000000000000000000000000000000000000000000000000000000000000", opts).unwrap();
        std::io::Write::write_all(&mut w, &vec![0u8; 32 * 1024 * 1024]).unwrap();
        w.finish().unwrap();
    }
    let bomb = cursor.into_inner();
    assert!(bomb.len() < 1024 * 1024, "the bomb itself must be small: {} bytes", bomb.len());

    let res = app.client.post(app.url("/import")).body(bomb).send().await.unwrap();
    assert_eq!(res.status(), 413, "{}", res.text().await.unwrap());

    // Nothing was written: the archive never reached the import transaction.
    let objects: serde_json::Value = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objects.as_array().unwrap().len(), 0, "{objects}");
}

/// The archive body itself is bounded by `max_import_mb` (4 MiB in tests), independent of how
/// well it compresses.
#[tokio::test]
async fn import_rejects_an_oversized_archive_body() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    // Incompressible random-ish bytes, so the stored archive really is over the body limit.
    let big: Vec<u8> = (0..5 * 1024 * 1024u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    let res = app.client.post(app.url("/import")).body(big).send().await.unwrap();
    assert_eq!(res.status(), 413, "{}", res.text().await.unwrap());
}

/// `snoozed_until` is wired through the export archive and the import INSERT, but nothing
/// exercised it end to end: it must survive a real export/import round trip alongside the
/// reminder it suppresses.
#[tokio::test]
async fn export_round_trips_a_snoozed_reminder() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2026-01-01", "category": "maintenance", "title": "service", "counter_value": 60_000 }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Service", "due_counter": 60_000 }))
        .send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let snoozed: serde_json::Value = res.json().await.unwrap();
    let expected = snoozed["snoozed_until"].as_str().unwrap().to_string();

    let zip_bytes = app.client.get(app.url("/export")).send().await.unwrap().bytes().await.unwrap().to_vec();

    let anna = app.create_user_client("anna", "password123").await;
    let res = anna.post(app.url("/import")).header("content-type", "application/zip").body(zip_bytes).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    let nid = objs[0]["id"].as_i64().unwrap();
    let rems: Vec<serde_json::Value> = anna.get(app.url(&format!("/objects/{nid}/reminders"))).send().await.unwrap().json().await.unwrap();
    let imported = rems.iter().find(|r| r["title"] == "Service").unwrap();
    assert_eq!(imported["snoozed_until"], expected, "snoozed_until must survive export and import");
    assert_eq!(imported["due"], false, "the imported reminder must still be suppressed");
}

/// `snoozed_until` was added after version-1 archives already existed in the wild --
/// `#[serde(default)]` on `ReminderExport::snoozed_until` is what lets those older archives
/// (which never wrote the field at all) still import.
#[tokio::test]
async fn import_succeeds_without_a_snoozed_until_field() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;

    let mut object = base_object();
    object["reminders"] = json!([{
        "title": "Oil", "notes": "", "due_date": "2020-01-01", "due_counter": null,
        "repeat_months": null, "repeat_counter": null, "done_at": null,
        "done_activity_index": null, "created_at": "2024-01-01T00:00:00Z"
        // no "snoozed_until" key at all -- exactly what a pre-snooze archive looked like.
    }]);
    let zip_bytes = zip_data_json(&export_shell(object));

    let res = anna.post(app.url("/import")).header("content-type", "application/zip").body(zip_bytes).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    let id = objs[0]["id"].as_i64().unwrap();
    let rems: Vec<serde_json::Value> = anna.get(app.url(&format!("/objects/{id}/reminders"))).send().await.unwrap().json().await.unwrap();
    assert!(rems[0]["snoozed_until"].is_null(), "a missing field must default to not-snoozed");
    assert_eq!(rems[0]["due"], true, "and the reminder must behave as never snoozed");
}

/// A column the archive does not carry is a column a restore silently erases -- fuel
/// quantity and fuel unit must round-trip through export and import like every other field.
#[tokio::test]
async fn export_round_trips_fuel_quantity() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "l"
    })).send().await.unwrap();
    let car: serde_json::Value = res.json().await.unwrap();
    let id = car["id"].as_i64().unwrap();
    app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-03-05", "category": "fuel", "title": "Fuel",
        "counter_value": 12_000, "quantity_milli": 41_300
    })).send().await.unwrap();

    let zip = app.client.get(app.url("/export")).send().await.unwrap().bytes().await.unwrap();

    let fresh = common::spawn().await;
    fresh.setup("ben", "correct horse").await;
    let res = fresh.client.post(fresh.url("/import"))
        .header("content-type", "application/zip")
        .body(zip.to_vec())
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objects: Vec<serde_json::Value> = fresh.client.get(fresh.url("/objects"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(objects[0]["fuel_unit"], "l");
    let nid = objects[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = fresh.client.get(fresh.url(&format!("/objects/{nid}/activities")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(acts[0]["quantity_milli"], 41_300, "the archive must not drop the quantity");
}

/// The archive is built into a scratch file and streamed back; the scratch file must not
/// survive the request, and the response must still be a complete, readable zip.
#[tokio::test]
async fn export_streams_and_leaves_no_scratch_file_behind() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    app.client.post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(Form::new().part("file", Part::bytes(png()).file_name("a.png").mime_str("image/png").unwrap()))
        .send().await.unwrap();

    let res = app.client.get(app.url("/export")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let declared: u64 = res.headers()["content-length"].to_str().unwrap().parse().unwrap();
    let bytes = res.bytes().await.unwrap().to_vec();
    assert_eq!(bytes.len() as u64, declared, "Content-Length must match what was streamed");

    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(z.by_name("data.json").is_ok());
    assert!(z.len() >= 2, "the photo blob rides along: {} entries", z.len());

    // `data/files` holds only sharded blob directories -- no leftover scratch archive.
    let files_dir = app.state.storage.blob_path(&"0".repeat(64)).parent().unwrap().parent().unwrap().to_path_buf();
    let leftovers: Vec<_> = std::fs::read_dir(&files_dir).unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with('.') || n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "scratch files left behind: {leftovers:?}");
}
