use super::common;
use super::helpers::*;
use serde_json::json;

#[tokio::test]
async fn purge_drops_old_log_rows_and_old_tombstones() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(object_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    app.client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-ancient", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Ancient",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();

    // Backdate both the log row and a tombstone well past any sane window.
    sqlx::query("UPDATE changes SET applied_at = '2000-01-01T00:00:00Z'")
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE objects SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = $1")
        .bind(object_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    let removed = logb::sync::feed::purge(&app.state, 90).await.unwrap();
    // The blanket backdate above ages out every `changes` row for this user, including the
    // object's own `create` (task 9 logs REST creates too), not only the pushed `set`.
    assert_eq!(removed, 2, "the ancient log rows went");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(rows, 0);

    let objects: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(object_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(objects, 0, "an expired tombstone is finally a real delete");
}

/// The purge must not hard-delete a tombstoned parent while any object -- live, or itself
/// tombstoned but not yet purged -- still names it as `parent_id`. `objects.parent_id`
/// references `objects(id)` with no `ON DELETE` action, so taking the parent first, in any purge
/// run where the child's own row survives that same statement -- its tombstone still fresh, or
/// the child itself held back by one of the other three guards -- fails the entire purge on the
/// foreign key. An ordinary cascade delete tombstones a whole subtree under one shared
/// timestamp, so parent and child usually age out and get purged together in the same statement,
/// where no violation occurs; this guard exists for the case where they don't, and that is the
/// case this test stages by backdating the parent's tombstone alone. Were the reference ever
/// relaxed it would instead leave the child pointing at a row that no longer exists. Either way
/// the parent waits, exactly as it already waits for its activities, reminders and attachments.
#[tokio::test]
async fn a_tombstoned_parent_is_not_purged_while_a_tombstoned_child_still_references_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    let garage_id = garage["id"].as_i64().unwrap();
    let light_id = light["id"].as_i64().unwrap();

    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2")
        .bind(garage_id)
        .bind(light_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    // The REST delete tombstones the garage and cascades a tombstone onto the light.
    app.delete_object(&garage).await;
    // Backdate the parent's tombstone alone -- the same date `age_out_tombstones` uses, applied
    // to one row -- so the parent is eligible for this run and the child, whose tombstone is
    // minutes old, is not. That is the only arrangement that puts a real foreign key check
    // between two rows: with both eligible they would go in one statement, where a no-action
    // constraint is checked at the end and sees nothing wrong.
    sqlx::query("UPDATE objects SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = $1")
        .bind(garage_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    app.run_purge().await;

    let parent: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(garage_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        parent, 1,
        "the parent must survive while a child still names it"
    );
    let child: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(light_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        child, 1,
        "the child's tombstone is still inside the window and stays"
    );

    // Once the child has aged out too it goes, and the guard -- which reads the table as the
    // statement found it -- still holds the parent back for that run, so the parent leaves on
    // the next one. That is the guard's whole cost: one extra run per level of nesting.
    app.age_out_tombstones().await;
    app.run_purge().await;
    let child: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(light_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(child, 0, "the aged-out child goes on this run");

    app.run_purge().await;
    let parent: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(garage_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        parent, 0,
        "once nothing names it the parent is finally purged"
    );
}

/// The purge's one silent forever-retention, said out loud.
///
/// The `objects` guard asks whether any row still names this one as its parent, and carries no
/// `c.id <> objects.id`, so a row that is its own parent answers its own guard on every run and
/// is never purged. No validated write path can produce one -- both doors go through
/// `record::parent_is_valid`, and `objects::update` asks it under the write lock -- so this is
/// an import or a hand edit, and the row is planted here the same way. Being held back is the
/// safe outcome and is not what this test changes; what it pins is that an operator can *see*
/// it, rather than a tombstone quietly outliving its retention window with no error and no log
/// line. Delete the `warn!` in `sync::feed::purge` and this fails while the retention assertion
/// above it still passes.
#[tokio::test]
async fn a_self_parenting_tombstone_is_held_back_and_says_so() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let orphan = app.create_object(&app.client, "Ouroboros", None).await;
    let id = orphan["id"].as_i64().unwrap();
    app.delete_object(&orphan).await;
    // Past the validation, exactly as an import or a hand-edited database could.
    sqlx::query("UPDATE objects SET parent_id = id WHERE id = $1")
        .bind(id)
        .execute(&app.state.db)
        .await
        .unwrap();
    app.age_out_tombstones().await;

    app.run_purge().await;

    let still_there: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        still_there, 1,
        "the guard holds a self-parenting row back, which is the safe half"
    );
    let logs = app.captured_logs();
    assert!(
        logs.contains("name themselves as their own parent"),
        "the purge must warn about a row it can never remove, not drop it silently",
    );
}

#[tokio::test]
async fn purge_keeps_recent_history() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    app.client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-fresh", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Fresh",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();

    assert_eq!(logb::sync::feed::purge(&app.state, 90).await.unwrap(), 0);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
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
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        201,
        "upload failed: {}",
        res.text().await.unwrap()
    );
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    let sha: String = sqlx::query_scalar(
        "SELECT f.sha256 FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = $1",
    )
    .bind(attachment_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let blob = app.state.storage.blob_path(&sha);
    assert!(blob.exists(), "the upload landed on disk");

    assert_eq!(
        app.client
            .delete(app.url(&format!("/attachments/{attachment_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert!(blob.exists(), "a tombstoned attachment still pins its blob");

    sqlx::query("UPDATE attachments SET deleted_at = '2000-01-01T00:00:00Z'")
        .execute(&app.state.db)
        .await
        .unwrap();
    logb::sync::feed::purge(&app.state, 90).await.unwrap();

    assert!(
        !blob.exists(),
        "an expired tombstone finally frees the bytes"
    );
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM files")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(files, 0, "the files row goes with its last attachment");
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
        let user_id: i64 = sqlx::query_scalar("SELECT user_id FROM objects WHERE id = $1")
            .bind(object_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();

        // A live clock the sweep must leave alone, so a run that swept everything -- rather
        // than only the orphan -- would still be caught.
        let res = app
            .client
            .post(app.url("/sync/push"))
            .json(&push_body(json!([{
                "client_op_id": "op-name", "entity": "object", "entity_uuid": &object_uuid,
                "op": "set", "field": "name", "value": "Renamed",
                "edited_at": after_now(7776060), "device_id": "phone"
            }])))
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            200,
            "push failed: {}",
            res.text().await.unwrap()
        );

        // A row from before `client_uuid` existed, or from any writer that never set it, in the
        // table under test this iteration -- every column each table's NOT NULL constraints
        // require, and nothing that names `client_uuid`, so it defaults NULL.
        match legacy_table {
            "objects" => {
                sqlx::query(
                    "INSERT INTO objects (user_id, name, type, created_at, updated_at) \
                     VALUES ($1, 'Legacy', 'car', '2026-03-05T00:00:00Z', '2026-03-05T00:00:00Z')",
                )
                .bind(user_id)
                .execute(&app.state.db)
                .await
                .unwrap();
            }
            "activities" => {
                sqlx::query(
                    "INSERT INTO activities \
                     (object_id, date, category, title, notes, created_at, updated_at) \
                     VALUES ($1, '2026-03-05', 'other', 'Legacy', '', \
                             '2026-03-05T00:00:00Z', '2026-03-05T00:00:00Z')",
                )
                .bind(object_id)
                .execute(&app.state.db)
                .await
                .unwrap();
            }
            "reminders" => {
                sqlx::query(
                    "INSERT INTO reminders (object_id, title, due_date, created_at) \
                     VALUES ($1, 'Legacy', '2026-09-01', '2026-03-05T00:00:00Z')",
                )
                .bind(object_id)
                .execute(&app.state.db)
                .await
                .unwrap();
            }
            "attachments" => {
                let file_id: i64 = sqlx::query_scalar(
                    "INSERT INTO files (user_id, sha256, original_name, mime, size, created_at) \
                     VALUES ($1, 'deadbeef', 'legacy.png', 'image/png', 1, '2026-03-05T00:00:00Z') \
                     RETURNING id",
                )
                .bind(user_id)
                .fetch_one(&app.state.db)
                .await
                .unwrap();
                sqlx::query(
                    "INSERT INTO attachments (object_id, file_id, kind, created_at) \
                     VALUES ($1, $2, 'photo', '2026-03-05T00:00:00Z')",
                )
                .bind(object_id)
                .bind(file_id)
                .execute(&app.state.db)
                .await
                .unwrap();
            }
            "files" => {
                sqlx::query(
                    "INSERT INTO files (user_id, sha256, original_name, mime, size, created_at) \
                     VALUES ($1, 'deadbeef', 'legacy.png', 'image/png', 1, '2026-03-05T00:00:00Z')",
                )
                .bind(user_id)
                .execute(&app.state.db)
                .await
                .unwrap();
            }
            other => unreachable!("not one of the five tables: {other}"),
        }

        // A clock for a uuid no table carries any more: the sweep's whole reason to exist.
        sqlx::query(
            "INSERT INTO field_clock (entity, entity_uuid, field, edited_at, device_id) \
             VALUES ('activity', 'gone-with-the-row', 'title', '2026-01-01T00:00:00Z', 'phone')",
        )
        .execute(&app.state.db)
        .await
        .unwrap();

        logb::sync::feed::purge(&app.state, 90).await.unwrap();

        let orphans: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM field_clock WHERE entity_uuid = 'gone-with-the-row'",
        )
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        assert_eq!(
            orphans, 0,
            "the orphaned clock must be swept even beside a NULL client_uuid in {legacy_table}"
        );

        let kept: i64 =
            sqlx::query_scalar("SELECT count(*) FROM field_clock WHERE entity_uuid = $1")
                .bind(&object_uuid)
                .fetch_one(&app.state.db)
                .await
                .unwrap();
        // 20, not 1: the object's own REST `create` stamps every field in `Entity::Object`'s
        // whitelist (task 9), and the pushed `set` above only overwrites `name`'s entry rather
        // than adding a fourteenth. All 13 must survive the sweep untouched. It was 9 until
        // `parent_id` joined the whitelist, 10 until `tags` did, and 11 until `energy_price_milli`
        // did, 12 until weight_unit did, 14 with fuel_capacity_milli, and 20 with the generic
        // resource settings -- this count is deliberately a literal so that widening the whitelist has to be
        // noticed here.
        assert_eq!(
            kept, 20,
            "a clock for a row that still exists must be left alone (NULL planted in {legacy_table})"
        );
    }
}
