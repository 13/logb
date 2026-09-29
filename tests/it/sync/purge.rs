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

async fn clocks_for(app: &common::TestApp, uuid: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM field_clock WHERE entity_uuid = $1")
        .bind(uuid)
        .fetch_one(&app.state.db)
        .await
        .unwrap()
}

/// The purge forgets the field clocks of exactly the rows it removes: a purged object's and a
/// purged activity's go, a live object's all stay, and so does a tombstone's that is still
/// inside the window. It no longer sweeps the whole table for clocks that name nothing, so a
/// row without a `client_uuid` anywhere -- which once disabled that sweep outright -- cannot
/// get in the way; one is planted here to keep it that way.
#[tokio::test]
async fn the_purge_forgets_the_field_clocks_of_what_it_removes_and_nothing_else() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let live = app.create_object(&app.client, "Golf", Some("km")).await;
    let live_uuid = client_uuid(&app.state.db, "objects", live["id"].as_i64().unwrap()).await;
    let gone = app.create_object(&app.client, "Polo", Some("km")).await;
    let gone_uuid = client_uuid(&app.state.db, "objects", gone["id"].as_i64().unwrap()).await;
    let activity = app.create_activity(&live["id"], "Oil").await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity["id"].as_i64().unwrap()).await;
    let fresh = app.create_activity(&live["id"], "Tyres").await;
    let fresh_uuid = client_uuid(&app.state.db, "activities", fresh["id"].as_i64().unwrap()).await;
    for uuid in [&live_uuid, &gone_uuid, &activity_uuid, &fresh_uuid] {
        assert!(clocks_for(&app, uuid).await > 0, "a REST create stamps its fields");
    }
    let live_clocks = clocks_for(&app, &live_uuid).await;

    // A row from before `client_uuid` existed.
    sqlx::query(
        "INSERT INTO activities (object_id, date, category, title, notes, created_at, updated_at) \
         VALUES ($1, '2026-03-05', 'other', 'Legacy', '', '2026-03-05T00:00:00Z', '2026-03-05T00:00:00Z')",
    )
    .bind(live["id"].as_i64().unwrap())
    .execute(&app.state.db)
    .await
    .unwrap();

    app.delete_object(&gone).await;
    let res = app.client.delete(app.url(&format!("/activities/{}", activity["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204);
    app.age_out_tombstones().await;
    // Deleted after the backdate, so still inside the window: its row and its clocks stay.
    let res = app.client.delete(app.url(&format!("/activities/{}", fresh["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204);
    app.run_purge().await;

    assert_eq!(clocks_for(&app, &gone_uuid).await, 0, "the purged object's clocks went with it");
    assert_eq!(clocks_for(&app, &activity_uuid).await, 0, "the purged activity's clocks went with it");
    assert_eq!(clocks_for(&app, &live_uuid).await, live_clocks, "a live row's clocks are untouched");
    assert!(clocks_for(&app, &fresh_uuid).await > 0, "a tombstone inside the window keeps its clocks");
}

/// What the old hourly sweep would have found, migration 0029 removes once: clocks naming a uuid
/// no row carries. Beside a row with no `client_uuid` at all, which is what once turned a
/// `NOT IN` version of that sweep into a no-op. SQLite only and without the harness, like
/// `migration_object_types`: it replays the migration files onto an in-memory database, and
/// the PostgreSQL file runs the same statement.
#[tokio::test]
async fn migration_0029_clears_the_clocks_that_name_nothing() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().in_memory(true).foreign_keys(true))
        .await
        .unwrap();
    let mut files: Vec<String> = std::fs::read_dir("migrations/sqlite")
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.as_str() < "0029")
        .collect();
    files.sort();
    for file in &files {
        let sql = std::fs::read_to_string(format!("migrations/sqlite/{file}")).unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(&pool).await.unwrap();
    }
    sqlx::raw_sql(
        "INSERT INTO users (id, username, password_hash, is_admin, created_at) \
         VALUES (1, 'ben', 'x', 1, '2026-01-01T00:00:00Z');
         INSERT INTO objects (id, user_id, name, type, description, created_at, updated_at, client_uuid) \
         VALUES (1, 1, 'Golf', 'car', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'live-object');
         INSERT INTO objects (id, user_id, name, type, description, created_at, updated_at) \
         VALUES (2, 1, 'Legacy', 'car', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
         INSERT INTO field_clock (entity, entity_uuid, field, edited_at, device_id) VALUES \
           ('object', 'live-object', 'name', '2026-01-01T00:00:00Z', 'phone'), \
           ('activity', 'gone-with-the-row', 'title', '2026-01-01T00:00:00Z', 'phone');",
    )
    .execute(&pool)
    .await
    .unwrap();

    let sql = std::fs::read_to_string("migrations/sqlite/0029_perf_indexes.sql").unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(&pool).await.unwrap();

    let left: Vec<String> = sqlx::query_scalar("SELECT entity_uuid FROM field_clock ORDER BY entity_uuid")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(left, ["live-object"]);
}
