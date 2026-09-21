mod common;
use reqwest::multipart::{Form, Part};
use serde_json::json;

/// Zips a single `data.json` entry containing `data`, as a real export archive would.
fn zip_data_json(data: &serde_json::Value) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut cursor);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
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
    let serde_json::Value::Object(map) = fields else {
        panic!("fields must be a JSON object")
    };
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

    let res = app
        .client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app
        .client
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objs[0]["type"], "car");
}

/// Text that never mapped to a type is not discarded -- it is appended to the description on
/// its own line, exactly what the migration's `CASE` did for the rows already on disk.
#[tokio::test]
async fn unmapped_text_in_an_old_archive_reaches_the_description() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(json!({ "name": "Odd", "category": "Gravelbike Custom" }));

    let res = app
        .client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app
        .client
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objs[0]["type"], "other");
    assert_eq!(objs[0]["description"], "Gravelbike Custom");
}

/// The migration's description `CASE` trims only `category`, never `description` -- a
/// description with surrounding whitespace keeps it, the same way `description || char(10) ||
/// trim(category)` does in SQL. Import must match: trimming `description` here would make a
/// restored backup disagree with a migrated database about the same row.
#[tokio::test]
async fn unmapped_category_preserves_padding_already_in_the_description() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(json!({
        "name": "Odd", "category": "Gravelbike Custom", "description": "  padded desc  "
    }));

    let res = app
        .client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app
        .client
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objs[0]["type"], "other");
    assert_eq!(objs[0]["description"], "  padded desc  \nGravelbike Custom");
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

    let res = app
        .client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app
        .client
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objs[0]["type"], "bike");
}

/// A `type` this build does not recognise (a typo, or a future value) is not guessed at either
/// -- it falls back to `other`, same as an unmapped legacy word. Unlike the previous behaviour,
/// the string itself is not discarded: an unrecognised descriptor is preserved the same way
/// whichever field it arrived in, so it is appended to the description on its own line, exactly
/// like an unmapped `category`.
#[tokio::test]
async fn an_illegal_type_falls_back_to_other_and_its_text_reaches_the_description() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(
        json!({ "type": "spaceship", "category": null, "description": "kept as is" }),
    );

    let res = app
        .client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app
        .client
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objs[0]["type"], "other");
    assert_eq!(objs[0]["description"], "kept as is\nspaceship");
}

/// `type` and `category` both present, and `type` illegal: `type`'s mere presence still wins
/// outright over `category` -- the same rule as when `type` is legal -- so the illegal `type`
/// text is what reaches the description, and `category` ("Auto", which would otherwise map to
/// `car`) is silently discarded rather than consulted as a second source.
#[tokio::test]
async fn an_illegal_type_wins_over_category_and_only_its_text_is_preserved() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(
        json!({ "type": "spaceship", "category": "Auto", "description": "kept as is" }),
    );

    let res = app
        .client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = app
        .client
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objs[0]["type"], "other");
    assert_eq!(objs[0]["description"], "kept as is\nspaceship");
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
    // A non-default type ("bike", not "car") so the type assertion below can actually fail --
    // a broken round trip that always lands on the first `OBJECT_TYPES` entry would otherwise
    // pass it by accident.
    let obj: serde_json::Value = app
        .client
        .post(app.url("/objects"))
        .json(&json!({
            "name": "Golf", "type": "bike", "counter_unit": "km",
            "description": "", "purchase_date": null, "purchase_price_cents": null
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = obj["id"].as_i64().unwrap();
    let act: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2024-01-01", "category": "repair", "title": "Brakes", "cost_cents": 12345, "counter_value": 100 }))
        .send().await.unwrap().json().await.unwrap();
    let base = app.url(&format!("/objects/{id}/attachments"));
    app.client
        .post(&base)
        .multipart(
            Form::new()
                .part(
                    "file",
                    Part::bytes(png())
                        .file_name("a.png")
                        .mime_str("image/png")
                        .unwrap(),
                )
                .text("activity_id", act["id"].to_string()),
        )
        .send()
        .await
        .unwrap();
    app.client
        .post(&base)
        .multipart(
            Form::new().part(
                "file",
                Part::bytes(b"manual".to_vec())
                    .file_name("m.txt")
                    .mime_str("text/plain")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap();
    app.client
        .post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Oil", "due_counter": 5000, "repeat_counter": 5000 }))
        .send()
        .await
        .unwrap();
    let r: serde_json::Value = app
        .client
        .post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Done one", "due_date": "2020-01-01" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    app.client
        .post(app.url(&format!("/reminders/{}/done", r["id"])))
        .json(&json!({ "activity_id": act["id"] }))
        .send()
        .await
        .unwrap();

    let res = app.client.get(app.url("/export")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "application/zip");
    let zip_bytes = res.bytes().await.unwrap().to_vec();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes.clone())).unwrap();
    assert!(z.by_name("data.json").is_ok());
    // The icon rides along so an archive is recognisable as LogB's at a glance. It is the
    // embedded SPA asset, not a second copy -- see `export::icon_bytes`.
    assert!(
        z.by_name("icon.svg").is_ok(),
        "the archive carries the logo"
    );
    assert_eq!(z.len(), 4, "data.json + icon.svg + 2 blobs");

    // import into a different user
    let anna = app.create_user_client("anna", "password123").await;
    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let counts: serde_json::Value = res.json().await.unwrap();
    assert_eq!(counts["objects"], 1);
    assert_eq!(counts["activities"], 1);
    assert_eq!(counts["attachments"], 2);
    assert_eq!(counts["reminders"], 2);

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objs[0]["name"], "Golf");
    assert_eq!(objs[0]["type"], "bike");
    assert_eq!(objs[0]["stats"]["total_cost_cents"], 12345);
    let nid = objs[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{nid}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(acts[0]["attachments"][0]["original_name"], "a.png");
    let thumb = anna
        .get(app.url(&format!(
            "/files/{}/thumb",
            acts[0]["attachments"][0]["file_id"]
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(thumb.status(), 200);
    let rems: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{nid}/reminders")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let done = rems.iter().find(|r| r["title"] == "Done one").unwrap();
    assert_eq!(done["done_activity_id"], acts[0]["id"]);

    // single-object export
    let res = app
        .client
        .get(app.url(&format!("/export?object_id={id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let res = app
        .client
        .get(app.url("/export?object_id=9999"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(b"nope".to_vec())
        .send()
        .await
        .unwrap();
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

    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
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

    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objs[0]["name"], "Golf");
    assert_eq!(objs[0]["type"], "car");
    let id = objs[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{id}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(acts[0]["title"], "Brakes");
    let rems: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{id}/reminders")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
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

    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
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
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        w.start_file("data.json", opts).unwrap();
        std::io::Write::write_all(
            &mut w,
            serde_json::to_vec(&export_shell(base_object()))
                .unwrap()
                .as_slice(),
        )
        .unwrap();
        w.start_file(
            "files/0000000000000000000000000000000000000000000000000000000000000000",
            opts,
        )
        .unwrap();
        std::io::Write::write_all(&mut w, &vec![0u8; 32 * 1024 * 1024]).unwrap();
        w.finish().unwrap();
    }
    let bomb = cursor.into_inner();
    assert!(
        bomb.len() < 1024 * 1024,
        "the bomb itself must be small: {} bytes",
        bomb.len()
    );

    let res = app
        .client
        .post(app.url("/import"))
        .body(bomb)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 413, "{}", res.text().await.unwrap());

    // Nothing was written: the archive never reached the import transaction.
    let objects: serde_json::Value = app
        .client
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objects.as_array().unwrap().len(), 0, "{objects}");
}

/// The archive body itself is bounded by `max_import_mb` (4 MiB in tests), independent of how
/// well it compresses.
#[tokio::test]
async fn import_rejects_an_oversized_archive_body() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    // Incompressible random-ish bytes, so the stored archive really is over the body limit.
    let big: Vec<u8> = (0..5 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let res = app
        .client
        .post(app.url("/import"))
        .body(big)
        .send()
        .await
        .unwrap();
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
    let r: serde_json::Value = app
        .client
        .post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Service", "due_counter": 60_000, "schedule": "monthly:last" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rid = r["id"].as_i64().unwrap();

    let res = app
        .client
        .post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let snoozed: serde_json::Value = res.json().await.unwrap();
    let expected = snoozed["snoozed_until"].as_str().unwrap().to_string();

    let zip_bytes = app
        .client
        .get(app.url("/export"))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap()
        .to_vec();

    let anna = app.create_user_client("anna", "password123").await;
    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let nid = objs[0]["id"].as_i64().unwrap();
    let rems: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{nid}/reminders")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let imported = rems.iter().find(|r| r["title"] == "Service").unwrap();
    assert_eq!(
        imported["snoozed_until"], expected,
        "snoozed_until must survive export and import"
    );
    assert_eq!(imported["schedule"], "monthly:last", "calendar recurrence must survive export and import");
    assert_eq!(
        imported["due"], false,
        "the imported reminder must still be suppressed"
    );
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

    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = objs[0]["id"].as_i64().unwrap();
    let rems: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{id}/reminders")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        rems[0]["snoozed_until"].is_null(),
        "a missing field must default to not-snoozed"
    );
    assert_eq!(
        rems[0]["due"], true,
        "and the reminder must behave as never snoozed"
    );
}

/// A column the archive does not carry is a column a restore silently erases -- fuel
/// quantity and fuel unit must round-trip through export and import like every other field.
#[tokio::test]
async fn export_round_trips_fuel_quantity() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app
        .client
        .post(app.url("/objects"))
        .json(&json!({
            "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "l"
        }))
        .send()
        .await
        .unwrap();
    let car: serde_json::Value = res.json().await.unwrap();
    let id = car["id"].as_i64().unwrap();
    app.client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({
            "date": "2026-03-05", "category": "fuel", "title": "Fuel",
            "counter_value": 12_000, "quantity_milli": 41_300
        }))
        .send()
        .await
        .unwrap();

    let zip = app
        .client
        .get(app.url("/export"))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let fresh = common::spawn().await;
    fresh.setup("ben", "correct horse").await;
    let res = fresh
        .client
        .post(fresh.url("/import"))
        .header("content-type", "application/zip")
        .body(zip.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objects: Vec<serde_json::Value> = fresh
        .client
        .get(fresh.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objects[0]["fuel_unit"], "l");
    let nid = objects[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = fresh
        .client
        .get(fresh.url(&format!("/objects/{nid}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        acts[0]["quantity_milli"], 41_300,
        "the archive must not drop the quantity"
    );
}

/// A trip's five extra fields must survive export and import like `fuel_quantity` above -- an
/// archive that dropped them would silently turn every restored trip back into a bare reading.
#[tokio::test]
async fn export_round_trips_a_trip() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let id = bike["id"].as_i64().unwrap();
    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({
            "date": "2026-06-01", "category": "trip", "title": "", "notes": "",
            "start_counter": 400, "counter_value": 600, "from_place": "Home", "to_place": "Office",
            "duration_minutes": 75, "battery_used_pct": 32
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    let zip = app
        .client
        .get(app.url("/export"))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let fresh = common::spawn().await;
    fresh.setup("ben", "correct horse").await;
    let res = fresh
        .client
        .post(fresh.url("/import"))
        .header("content-type", "application/zip")
        .body(zip.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objects: Vec<serde_json::Value> = fresh
        .client
        .get(fresh.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let nid = objects[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = fresh
        .client
        .get(fresh.url(&format!("/objects/{nid}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(acts[0]["category"], "trip");
    assert_eq!(acts[0]["start_counter"], 400);
    assert_eq!(acts[0]["counter_value"], 600);
    assert_eq!(acts[0]["from_place"], "Home");
    assert_eq!(acts[0]["to_place"], "Office");
    assert_eq!(acts[0]["duration_minutes"], 75);
    assert_eq!(acts[0]["battery_used_pct"], 32);
}

/// The five trip fields were added after version-1 archives already existed in the wild --
/// `#[serde(default)]` on `ActivityExport`'s five new fields is what lets an older archive
/// (which never wrote any of them) still import, exactly as `import_succeeds_without_a_snoozed_until_field`
/// does for `snoozed_until`.
#[tokio::test]
async fn import_succeeds_without_trip_fields() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;

    let mut object = base_object();
    object["activities"] = json!([{
        "date": "2024-01-01", "category": "maintenance", "title": "Service", "notes": "",
        "counter_value": 1000, "cost_cents": null, "created_at": "2024-01-01T00:00:00Z",
        "attachments": []
        // no start_counter/from_place/to_place/duration_minutes/battery_used_pct at all --
        // exactly what a pre-trip archive looked like.
    }]);
    let zip_bytes = zip_data_json(&export_shell(object));

    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = objs[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{id}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for field in [
        "start_counter",
        "from_place",
        "to_place",
        "duration_minutes",
        "battery_used_pct",
    ] {
        assert!(
            acts[0][field].is_null(),
            "{field} must default to null: {}",
            acts[0]
        );
    }
}

/// `validate_import` trims and length-checks every place the same way a REST create does, but
/// used to discard the trimmed value: the archive's raw, untrimmed text reached the row. An
/// import must store what a REST create of the same body would, not what the archive happened
/// to spell.
#[tokio::test]
async fn import_trims_trip_places() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;

    let mut object = base_object();
    object["activities"] = json!([{
        "date": "2024-01-01", "category": "trip", "title": "", "notes": "",
        "counter_value": 600, "cost_cents": null, "created_at": "2024-01-01T00:00:00Z",
        "attachments": [], "start_counter": 400, "from_place": " Home ", "to_place": "   "
    }]);
    let zip_bytes = zip_data_json(&export_shell(object));

    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = objs[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{id}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        acts[0]["from_place"], "Home",
        "trimmed, not the archive's padded spelling"
    );
    assert_eq!(
        acts[0]["to_place"],
        serde_json::Value::Null,
        "blank after trimming stores null"
    );
}

/// Resource level, capacity, `charged_full` and `energy_price_milli` must survive export and import like the trip fields
/// above -- an archive that dropped them would silently turn a full charge back into an
/// ordinary fill, and forget a stored price per unit.
#[tokio::test]
async fn export_round_trips_charging_fields() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app
        .client
        .post(app.url("/objects"))
        .json(&json!({
            "name": "Oil tank", "type": "home", "counter_unit": "h", "fuel_unit": "l",
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "energy_price_milli": 30000, "fuel_capacity_milli": 2000000
        }))
        .send()
        .await
        .unwrap();
    let car: serde_json::Value = res.json().await.unwrap();
    let id = car["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-09-16", "category": "fuel", "title": "Fill", "notes": "", "charged_full": 1,
        "counter_value": 3420, "quantity_milli": 8500, "cost_cents": 255, "fuel_level_pct": 75
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    let zip = app
        .client
        .get(app.url("/export"))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let fresh = common::spawn().await;
    fresh.setup("ben", "correct horse").await;
    let res = fresh
        .client
        .post(fresh.url("/import"))
        .header("content-type", "application/zip")
        .body(zip.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objects: Vec<serde_json::Value> = fresh
        .client
        .get(fresh.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        objects[0]["energy_price_milli"], 30000,
        "the archive must not drop the price"
    );
    assert_eq!(
        objects[0]["fuel_capacity_milli"], 2000000,
        "the archive must not drop tank capacity"
    );
    let nid = objects[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = fresh
        .client
        .get(fresh.url(&format!("/objects/{nid}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        acts[0]["charged_full"], 1,
        "the archive must not drop the full-charge flag"
    );
    assert_eq!(
        acts[0]["fuel_level_pct"], 75,
        "the archive must not drop the tank level"
    );
}

/// `charged_full` and `energy_price_milli` were added after version-1 archives already existed
/// in the wild -- `#[serde(default)]` on `ActivityExport`/`ObjectExport` is what lets an older
/// archive (which never wrote either key) still import, exactly as `import_succeeds_without_trip_fields`
/// does for the trip fields.
#[tokio::test]
async fn import_succeeds_without_charging_fields() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;

    let mut object = base_object();
    object["fuel_unit"] = json!("kwh");
    // No `energy_price_milli` key at all, and an activity with no `charged_full` key -- exactly
    // what a pre-charging archive looked like.
    object["activities"] = json!([{
        "date": "2024-01-01", "category": "fuel", "title": "Charge", "notes": "",
        "counter_value": 1000, "cost_cents": null, "created_at": "2024-01-01T00:00:00Z",
        "attachments": []
    }]);
    let zip_bytes = zip_data_json(&export_shell(object));

    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        objs[0]["energy_price_milli"].is_null(),
        "must default to null: {}",
        objs[0]
    );
    let id = objs[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{id}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(acts[0]["charged_full"], 0, "must default to 0: {}", acts[0]);
}

/// The archive is built into a scratch file and streamed back; the scratch file must not
/// survive the request, and the response must still be a complete, readable zip.
#[tokio::test]
async fn export_streams_and_leaves_no_scratch_file_behind() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    app.client
        .post(app.url(&format!("/objects/{id}/attachments")))
        .multipart(
            Form::new().part(
                "file",
                Part::bytes(png())
                    .file_name("a.png")
                    .mime_str("image/png")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap();

    let res = app.client.get(app.url("/export")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let declared: u64 = res.headers()["content-length"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    let bytes = res.bytes().await.unwrap().to_vec();
    assert_eq!(
        bytes.len() as u64,
        declared,
        "Content-Length must match what was streamed"
    );

    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(z.by_name("data.json").is_ok());
    assert!(
        z.len() >= 2,
        "the photo blob rides along: {} entries",
        z.len()
    );

    // `data/files` holds only sharded blob directories -- no leftover scratch archive.
    let files_dir = app
        .state
        .storage
        .blob_path(&"0".repeat(64))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let leftovers: Vec<_> = std::fs::read_dir(&files_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with('.') || n.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "scratch files left behind: {leftovers:?}"
    );
}

/// `/export` is scoped to one object and its own activities, attachments and reminders -- a
/// parent living outside the exported object is never in the archive. Carrying `parent_id`
/// across an export would point at nothing on the far side, or at whatever unrelated row an
/// id-reassigning import happens to give a completely different object. An imported object
/// must always land with no parent, regardless of what it had.
#[tokio::test]
async fn an_exported_objects_parent_is_dropped_and_reimports_with_none() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let res = app
        .client
        .patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(
            &json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let moved: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        moved["parent_id"], house["id"],
        "the object really has a parent to lose"
    );

    let res = app
        .client
        .get(app.url(&format!("/export?object_id={}", garage["id"])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let zip_bytes = res.bytes().await.unwrap().to_vec();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes.clone())).unwrap();
    let mut data_json = String::new();
    std::io::Read::read_to_string(&mut z.by_name("data.json").unwrap(), &mut data_json).unwrap();
    assert!(
        !data_json.contains("parent_id"),
        "parent_id must not appear in the archive at all: {data_json}"
    );

    let anna = app.create_user_client("anna", "password123").await;
    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let imported: Vec<serde_json::Value> = anna
        .get(app.url("/objects?all=true"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        imported.len(),
        1,
        "the archive held the one exported object, not its parent"
    );
    assert_eq!(imported[0]["name"], "Garage");
    assert_eq!(
        imported[0]["parent_id"],
        serde_json::Value::Null,
        "an imported object lands as a root"
    );
}

/// Exports the caller's whole library and returns the archive bytes.
async fn export_zip(app: &common::TestApp) -> Vec<u8> {
    let res = app.client.get(app.url("/export")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    res.bytes().await.unwrap().to_vec()
}

/// Creates an object and one entry, both tagged, and returns the object's id.
async fn tagged_object_with_entry(app: &common::TestApp) -> i64 {
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "car", "description": "", "tags": ["Lease", "winter"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": "2026-03-01", "category": "repair", "title": "Tyres", "notes": "", "tags": ["Winter", "tax 2026"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    id
}

#[tokio::test]
async fn tags_survive_export_and_import() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    tagged_object_with_entry(&app).await;
    let zip = export_zip(&app).await;

    let fresh = common::spawn().await;
    fresh.setup("ben", "correct horse").await;
    let res = fresh
        .client
        .post(fresh.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objects = fresh.get_json("/objects").await;
    assert_eq!(objects[0]["tags"], json!(["Lease", "winter"]));
    let nid = objects[0]["id"].as_i64().unwrap();
    let acts = fresh.get_json(&format!("/objects/{nid}/activities")).await;
    assert_eq!(acts[0]["tags"], json!(["Winter", "tax 2026"]));
}

/// Archives written before tags carry no `tags` key anywhere; they import as untagged.
#[tokio::test]
async fn an_archive_without_tags_imports_with_none() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    tagged_object_with_entry(&app).await;
    let zip = export_zip(&app).await;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    let mut data_json = String::new();
    std::io::Read::read_to_string(&mut z.by_name("data.json").unwrap(), &mut data_json).unwrap();
    let mut data: serde_json::Value = serde_json::from_str(&data_json).unwrap();
    fn strip_tags(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(map) => {
                map.remove("tags");
                map.values_mut().for_each(strip_tags);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip_tags),
            _ => {}
        }
    }
    strip_tags(&mut data);
    assert!(!data.to_string().contains("\"tags\""), "{data}");

    let fresh = common::spawn().await;
    fresh.setup("ben", "correct horse").await;
    let res = fresh
        .client
        .post(fresh.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_data_json(&data))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objects = fresh.get_json("/objects").await;
    assert_eq!(objects[0]["tags"], json!([]));
    let nid = objects[0]["id"].as_i64().unwrap();
    let acts = fresh.get_json(&format!("/objects/{nid}/activities")).await;
    assert_eq!(acts[0]["tags"], json!([]));
}

/// Import goes through the same normalising as every other write: a hand-edited archive's
/// spelling is tidied, and one over the limit is refused whole.
#[tokio::test]
async fn imported_tags_are_normalised_and_limits_answer_400() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(
        json!({ "type": "car", "category": null, "tags": [" Lease ", "lease", "winter"] }),
    );
    let res = app
        .client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    assert_eq!(
        app.get_json("/objects").await[0]["tags"],
        json!(["Lease", "winter"])
    );

    let anna = app.create_user_client("anna", "password123").await;
    let many: Vec<String> = (0..11).map(|i| format!("t{i}")).collect();
    let mut object = base_object();
    object["activities"] = json!([{
        "date": "2024-01-01", "category": "repair", "title": "Brakes", "notes": "",
        "counter_value": null, "cost_cents": null, "created_at": "2024-01-01T00:00:00Z", "attachments": [],
        "tags": many
    }]);
    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_data_json(&export_shell(object)))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    let text = res.text().await.unwrap();
    assert!(text.contains("activity 0") && text.contains("10"), "{text}");
    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(objs.len(), 0, "rejected import must not persist anything");
}

/// Reads `data.json` out of an export archive.
fn data_of(zip: &[u8]) -> serde_json::Value {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip.to_vec())).unwrap();
    let mut data_json = String::new();
    std::io::Read::read_to_string(&mut z.by_name("data.json").unwrap(), &mut data_json).unwrap();
    serde_json::from_str(&data_json).unwrap()
}

async fn import_as(
    app: &common::TestApp,
    client: &reqwest::Client,
    zip: Vec<u8>,
) -> serde_json::Value {
    let res = client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    res.json().await.unwrap()
}

#[tokio::test]
async fn own_types_survive_export_and_import() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let scooter = app.post_json("/types", &json!({ "name": "E-scooter", "icon": "e-bike", "categories": ["repair", "fuel"], "counter_unit": "km" })).await;
    app.post_json(
        "/objects",
        &json!({ "name": "Kick", "type": scooter["key"], "counter_unit": "km" }),
    )
    .await;
    let zip = export_zip(&app).await;
    let data = data_of(&zip);
    assert_eq!(
        data["types"],
        json!([{
            "client_uuid": scooter["client_uuid"], "name": "E-scooter", "icon": "e-bike",
            "categories": ["repair", "fuel", "other"], "counter_unit": "km"
        }]),
        "{data}"
    );
    assert_eq!(data["objects"][0]["type"], scooter["key"]);

    // A fresh instance: the type arrives as it was, and the object uses it.
    let fresh = common::spawn().await;
    fresh.setup("ben", "correct horse").await;
    import_as(&fresh, &fresh.client, zip.clone()).await;
    let types = fresh.get_json("/types").await;
    assert_eq!(types.as_array().unwrap().len(), 1, "{types}");
    for field in ["name", "icon", "categories", "counter_unit", "key"] {
        assert_eq!(types[0][field], scooter[field], "{field}");
    }
    assert_eq!(fresh.get_json("/objects").await[0]["type"], types[0]["key"]);

    // Another account on the same instance: the uuid is taken, so the type gets a fresh one and
    // her object follows it.
    let anna = app.create_user_client("anna", "password123").await;
    import_as(&app, &anna, zip.clone()).await;
    let hers: serde_json::Value = anna
        .get(app.url("/types"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(hers.as_array().unwrap().len(), 1, "{hers}");
    assert_eq!(hers[0]["name"], "E-scooter");
    assert_ne!(hers[0]["client_uuid"], scooter["client_uuid"]);
    let her_objects: serde_json::Value = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(her_objects[0]["type"], hers[0]["key"]);
    assert_eq!(
        app.get_json("/types").await[0]["client_uuid"],
        scooter["client_uuid"],
        "ben's type is untouched"
    );

    // Ben restoring into his own account: the name already exists, so the import uses his type
    // rather than making a second one with the same name.
    import_as(&app, &app.client, zip).await;
    assert_eq!(app.get_json("/types").await.as_array().unwrap().len(), 1);
    let objects = app.get_json("/objects").await;
    assert_eq!(objects.as_array().unwrap().len(), 2);
    assert!(
        objects
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["type"] == scooter["key"]),
        "{objects}"
    );
}

#[tokio::test]
async fn an_archive_without_types_imports() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let zip = archive_with_object_json(json!({ "type": "bike", "category": null }));
    assert!(data_of(&zip).get("types").is_none());
    let counts = import_as(&app, &app.client, zip).await;
    assert_eq!(app.get_json("/objects").await[0]["type"], "bike");
    assert_eq!(app.get_json("/types").await, json!([]));
    // Nothing in `types` to create or fold onto an existing one: both counts stay at zero
    // rather than, say, miscounting the objects' own implicit type.
    assert_eq!(counts["types_created"], 0, "{counts}");
    assert_eq!(counts["types_merged"], 0, "{counts}");
}

/// One archive type folds onto a name the account already has (same name, different case, so
/// the merge is exercised on `tags::fold` and not a literal string match); the other is new to
/// the account. The response has to tell the two apart, or a client restoring a big backup has
/// no way to show "12 new types, 3 already had a match" -- it only knows "15 types" happened.
#[tokio::test]
async fn import_reports_how_many_types_were_created_and_how_many_were_merged() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.post_json("/types", &json!({ "name": "scooter", "icon": "e-bike", "categories": ["repair"], "counter_unit": "km" })).await;

    let mut data = export_shell(base_object());
    data["objects"] = json!([]);
    data["types"] = json!([
        // Same name as the account's existing type, differently cased: must merge, not create.
        { "client_uuid": uuid::Uuid::new_v4().to_string(), "name": "Scooter", "icon": "e-bike", "categories": ["repair"], "counter_unit": "km" },
        // A name the account has never seen: must create.
        { "client_uuid": uuid::Uuid::new_v4().to_string(), "name": "Trailer", "icon": "car", "categories": ["repair"], "counter_unit": "km" },
    ]);
    let counts = import_as(&app, &app.client, zip_data_json(&data)).await;
    assert_eq!(counts["types_created"], 1, "{counts}");
    assert_eq!(counts["types_merged"], 1, "{counts}");

    let types = app.get_json("/types").await;
    assert_eq!(
        types.as_array().unwrap().len(),
        2,
        "the merged type is not duplicated: {types}"
    );
}

/// The same fold applies WITHIN one archive, not only against the account's pre-existing types:
/// the loop re-reads `object_types` on every iteration, so a type it just inserted is "live" for
/// the very next one. An empty account importing "Trailer" then "trailer" must create the first
/// and merge the second onto it, not create two types that differ only by case.
#[tokio::test]
async fn two_archive_types_folding_to_each_other_create_one_and_merge_the_other() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let mut data = export_shell(base_object());
    data["objects"] = json!([]);
    data["types"] = json!([
        { "client_uuid": uuid::Uuid::new_v4().to_string(), "name": "Trailer", "icon": "car", "categories": ["repair"], "counter_unit": "km" },
        { "client_uuid": uuid::Uuid::new_v4().to_string(), "name": "trailer", "icon": "car", "categories": ["repair"], "counter_unit": "km" },
    ]);
    let counts = import_as(&app, &app.client, zip_data_json(&data)).await;
    assert_eq!(counts["types_created"], 1, "{counts}");
    assert_eq!(counts["types_merged"], 1, "{counts}");

    let types = app.get_json("/types").await;
    assert_eq!(
        types.as_array().unwrap().len(),
        1,
        "the two archive types must fold to one: {types}"
    );
}

#[tokio::test]
async fn an_unknown_custom_key_imports_as_other_with_the_key_noted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let key = format!("custom:{}", uuid::Uuid::new_v4());
    let zip =
        archive_with_object_json(json!({ "type": key, "category": null, "description": "blue" }));
    import_as(&app, &app.client, zip).await;
    let object = &app.get_json("/objects").await[0];
    assert_eq!(object["type"], "other");
    assert_eq!(object["description"], format!("blue\n{key}"));

    // A type in the archive that breaks the rules refuses the whole import.
    let mut data = export_shell(base_object());
    data["types"] = json!([{ "client_uuid": uuid::Uuid::new_v4().to_string(), "name": "Boat", "icon": "rocket", "categories": ["repair"], "counter_unit": null }]);
    let res = app
        .client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_data_json(&data))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    assert!(res.text().await.unwrap().contains("icon"));
    assert_eq!(app.get_json("/types").await, json!([]));
}

/// Two types in one archive with the same uuid (in any case) would be one key for two types, and
/// nothing says which one the objects mean. The import is refused, naming the type, before any
/// row is written.
#[tokio::test]
async fn an_archive_with_one_uuid_on_two_types_is_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let uuid = uuid::Uuid::new_v4().to_string();
    let mut data = export_shell(base_object());
    data["types"] = json!([
        { "client_uuid": uuid, "name": "Boat", "icon": "tool", "categories": ["repair"], "counter_unit": null },
        { "client_uuid": uuid.to_uppercase(), "name": "Canoe", "icon": "tool", "categories": ["repair"], "counter_unit": null },
    ]);
    let res = app
        .client
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip_data_json(&data))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    let text = res.text().await.unwrap();
    assert!(
        text.contains("Canoe") && text.contains("client_uuid"),
        "{text}"
    );
    assert_eq!(app.get_json("/types").await, json!([]));
    assert_eq!(app.get_json("/objects").await, json!([]));
}
