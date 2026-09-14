//! A user's own object types: CRUD, the rules on each field, and that an object may use only
//! its owner's types.

mod common;
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::AssertSqlSafe;

async fn post(app: &common::TestApp, client: &reqwest::Client, path: &str, body: Value) -> reqwest::Response {
    client.post(app.url(path)).json(&body).send().await.unwrap()
}

fn scooter() -> Value {
    json!({ "name": " E-scooter ", "icon": "e-bike", "categories": ["repair", "fuel"], "counter_unit": "km" })
}

/// Creates the scooter type as `client` and answers its JSON.
async fn create_scooter(app: &common::TestApp, client: &reqwest::Client) -> Value {
    let res = post(app, client, "/types", scooter()).await;
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json().await.unwrap()
}

#[tokio::test]
async fn create_list_update_and_delete_a_type() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let created = create_scooter(&app, &app.client).await;
    assert_eq!(created["name"], "E-scooter");
    assert_eq!(created["icon"], "e-bike");
    assert_eq!(created["categories"], json!(["repair", "fuel", "other"]));
    assert_eq!(created["counter_unit"], "km");
    let uuid = created["client_uuid"].as_str().unwrap();
    assert_eq!(created["key"], format!("custom:{uuid}"));
    let id = created["id"].as_i64().unwrap();

    let listed = app.get_json("/types").await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["key"], created["key"]);

    let res = app.client.patch(app.url(&format!("/types/{id}")))
        .json(&json!({ "name": "E-Scooter", "icon": "e-bike", "categories": ["repair"], "counter_unit": null }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "renaming to a different case of its own name is allowed");
    let updated: Value = res.json().await.unwrap();
    assert_eq!(updated["name"], "E-Scooter");
    assert_eq!(updated["categories"], json!(["repair", "other"]));
    assert_eq!(updated["counter_unit"], Value::Null);
    assert_eq!(updated["key"], created["key"], "the key never changes");

    let res = app.client.delete(app.url(&format!("/types/{id}"))).send().await.unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(app.get_json("/types").await, json!([]));
}

#[tokio::test]
async fn names_are_unique_ignoring_case_and_accents() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    create_scooter(&app, &app.client).await;
    for name in ["e-SCOOTER", "É-scooter"] {
        let res = post(&app, &app.client, "/types", json!({ "name": name, "icon": "box", "categories": ["repair"] })).await;
        assert_eq!(res.status(), 400, "{name}");
    }
    // Renaming another type onto the name is the same collision.
    let boat = app.post_json("/types", &json!({ "name": "Boat", "icon": "box", "categories": ["repair"] })).await;
    let res = app.client.patch(app.url(&format!("/types/{}", boat["id"])))
        .json(&json!({ "name": "e-scooter", "icon": "box", "categories": ["repair"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 400);
    // Types are per user: someone else may have their own E-scooter.
    let anna = app.create_user_client("anna", "password123").await;
    assert_eq!(post(&app, &anna, "/types", scooter()).await.status(), 201);
}

#[tokio::test]
async fn invalid_icon_unit_or_category_is_400() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for body in [
        json!({ "name": "Boat", "icon": "settings", "categories": ["repair"] }),
        json!({ "name": "Boat", "icon": "box", "categories": ["repair"], "counter_unit": "nm" }),
        json!({ "name": "Boat", "icon": "box", "categories": ["sailing"] }),
        json!({ "name": "Boat", "icon": "box", "categories": [] }),
        json!({ "name": "   ", "icon": "box", "categories": ["repair"] }),
        json!({ "name": "x".repeat(41), "icon": "box", "categories": ["repair"] }),
    ] {
        let res = post(&app, &app.client, "/types", body.clone()).await;
        assert_eq!(res.status(), 400, "{body}");
    }
    assert_eq!(app.get_json("/types").await, json!([]));
}

#[tokio::test]
async fn an_object_can_use_its_owners_type_only() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let key = create_scooter(&app, &app.client).await["key"].as_str().unwrap().to_string();

    let res = post(&app, &app.client, "/objects", json!({ "name": "Kick", "type": key, "counter_unit": "km" })).await;
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let object: Value = res.json().await.unwrap();
    assert_eq!(object["type"], key);
    assert_eq!(app.get_json(&format!("/objects/{}", object["id"])).await["type"], key);

    let anna = app.create_user_client("anna", "password123").await;
    let res = post(&app, &anna, "/objects", json!({ "name": "Borrowed", "type": key })).await;
    assert_eq!(res.status(), 400, "another user's type");
    // Nor by editing an object of her own onto it.
    let hers = app.create_object(&anna, "Golf", None).await;
    let res = anna.patch(app.url(&format!("/objects/{}", hers["id"])))
        .json(&json!({ "name": "Golf", "type": key })).send().await.unwrap();
    assert_eq!(res.status(), 400, "another user's type on update");

    let made_up = format!("custom:{}", uuid::Uuid::new_v4());
    let res = post(&app, &app.client, "/objects", json!({ "name": "Ghost", "type": made_up })).await;
    assert_eq!(res.status(), 400, "a type that does not exist");
}

#[tokio::test]
async fn deleting_a_type_in_use_is_409_with_the_count() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let created = create_scooter(&app, &app.client).await;
    let (id, key) = (created["id"].as_i64().unwrap(), created["key"].clone());
    let mut objects = Vec::new();
    for name in ["One", "Two"] {
        objects.push(app.post_json("/objects", &json!({ "name": name, "type": key })).await);
    }

    let res = app.client.delete(app.url(&format!("/types/{id}"))).send().await.unwrap();
    assert_eq!(res.status(), 409);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"], "in_use");
    assert_eq!(body["count"], 2);

    for object in &objects {
        app.delete_object(object).await;
    }
    let res = app.client.delete(app.url(&format!("/types/{id}"))).send().await.unwrap();
    assert_eq!(res.status(), 204, "deleted objects no longer count");
}

/// The rebuild in `migrations/sqlite/0014_own_types.sql` must leave every built-in type where
/// it was. Built from the migration files on an in-memory SQLite database, so it runs whichever
/// backend the suite points at; the PostgreSQL half is a plain `DROP CONSTRAINT`.
#[tokio::test]
async fn migration_keeps_built_in_types() {
    let opts = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await.unwrap();
    let mut files: Vec<String> = std::fs::read_dir("migrations/sqlite").unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".sql"))
        .collect();
    files.sort();
    let run = |file: String| {
        let pool = pool.clone();
        async move {
            let sql = std::fs::read_to_string(format!("migrations/sqlite/{file}")).unwrap();
            sqlx::raw_sql(AssertSqlSafe(sql)).execute(&pool).await.unwrap_or_else(|e| panic!("{file}: {e}"));
        }
    };
    let (before, after): (Vec<_>, Vec<_>) = files.into_iter().partition(|f| f.as_str() < "0014_own_types.sql");
    for file in before { run(file).await; }
    sqlx::raw_sql(
        "INSERT INTO users (id, username, password_hash, is_admin, created_at) VALUES (1, 'ben', 'x', 1, 't');
         INSERT INTO objects (id, user_id, name, type, created_at, updated_at, client_uuid) VALUES
           (1, 1, 'Golf', 'car', 't', 't', 'u1'), (2, 1, 'Wheel', 'other', 't', 't', 'u2');
         UPDATE objects SET parent_id = 1 WHERE id = 2;
         INSERT INTO activities (id, object_id, date, category, title, created_at, updated_at)
           VALUES (1, 1, '2026-02-01', 'repair', 'Oil', 't', 't');
         INSERT INTO files (id, user_id, sha256, original_name, mime, size, created_at) VALUES (1, 1, 'abc', 'a', 'image/png', 1, 't');
         INSERT INTO attachments (id, object_id, activity_id, file_id, kind, caption, created_at) VALUES (1, 1, 1, 1, 'photo', 'a', 't');",
    ).execute(&pool).await.unwrap();
    for file in after { run(file).await; }

    let types: Vec<(i64, String, Option<i64>)> =
        sqlx::query_as("SELECT id, type, parent_id FROM objects ORDER BY id").fetch_all(&pool).await.unwrap();
    assert_eq!(types, vec![(1, "car".to_string(), None), (2, "other".to_string(), Some(1))]);
    let attachments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments").fetch_one(&pool).await.unwrap();
    assert_eq!(attachments, 1, "the rebuild must not cascade into children");
    // Built-in and custom keys both land; NOT NULL still holds.
    for ty in ["car", "custom:0f3c"] {
        sqlx::query("INSERT INTO objects (user_id, name, type, created_at, updated_at) VALUES (1, 'n', $1, 't', 't')")
            .bind(ty).execute(&pool).await.unwrap_or_else(|e| panic!("{ty}: {e}"));
    }
    assert!(sqlx::query("INSERT INTO objects (user_id, name, type, created_at, updated_at) VALUES (1, 'n', NULL, 't', 't')")
        .execute(&pool).await.is_err());

    // And through the app, on whichever backend the suite runs.
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert_eq!(app.create_object(&app.client, "Golf", Some("km")).await["type"], "car");
}

/// Builds an in-memory SQLite database migrated up to (excluding) 0014, seeded by `seed`, and
/// answers the pool plus the result of running 0014 and everything after it.
async fn migrate_0014_over(seed: &str) -> (sqlx::SqlitePool, Result<(), String>) {
    let opts = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await.unwrap();
    let mut files: Vec<String> = std::fs::read_dir("migrations/sqlite").unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".sql"))
        .collect();
    files.sort();
    let (before, after): (Vec<_>, Vec<_>) = files.into_iter().partition(|f| f.as_str() < "0014_own_types.sql");
    for file in before {
        let sql = std::fs::read_to_string(format!("migrations/sqlite/{file}")).unwrap();
        sqlx::raw_sql(AssertSqlSafe(sql)).execute(&pool).await.unwrap_or_else(|e| panic!("{file}: {e}"));
    }
    sqlx::raw_sql(AssertSqlSafe(seed.to_string())).execute(&pool).await.unwrap();
    for file in after {
        let sql = std::fs::read_to_string(format!("migrations/sqlite/{file}")).unwrap();
        if let Err(e) = sqlx::raw_sql(AssertSqlSafe(sql)).execute(&pool).await {
            return (pool, Err(format!("{file}: {e}")));
        }
    }
    (pool, Ok(()))
}

/// The rebuild runs with foreign keys off, so 0014 checks them itself before committing. A clean
/// database with parents, activities and attachments migrates; one whose children already point
/// at nothing makes the migration fail rather than commit silently.
#[tokio::test]
async fn the_rebuild_checks_foreign_keys_before_committing() {
    let (pool, result) = migrate_0014_over(
        "INSERT INTO users (id, username, password_hash, is_admin, created_at) VALUES (1, 'ben', 'x', 1, 't');
         INSERT INTO objects (id, user_id, name, type, created_at, updated_at, client_uuid) VALUES
           (1, 1, 'House', 'home', 't', 't', 'u1'), (2, 1, 'Boiler', 'appliance', 't', 't', 'u2');
         UPDATE objects SET parent_id = 1 WHERE id = 2;
         INSERT INTO activities (id, object_id, date, category, title, created_at, updated_at)
           VALUES (1, 2, '2026-02-01', 'repair', 'Valve', 't', 't');
         INSERT INTO files (id, user_id, sha256, original_name, mime, size, created_at) VALUES (1, 1, 'abc', 'a', 'image/png', 1, 't');
         INSERT INTO attachments (id, object_id, activity_id, file_id, kind, caption, created_at) VALUES (1, 2, 1, 1, 'photo', 'a', 't');
         INSERT INTO changes (seq, entity, entity_uuid, op, edited_at, applied_at, user_id, device_id, client_op_id)
           VALUES (6, 'object', 'u1', 'create', 't', 't', 1, 'rest', 'op-6'), (7, 'object', 'u2', 'create', 't', 't', 1, 'rest', 'op-7');
         DELETE FROM changes WHERE seq = 7;",
    ).await;
    result.expect("a consistent database migrates");
    let violations = sqlx::query("PRAGMA foreign_key_check").fetch_all(&pool).await.unwrap();
    assert!(violations.is_empty(), "{} foreign key violations after the rebuild", violations.len());
    // The rebuilt log keeps its rows and its counter (7 was handed out and purged), and takes
    // type writes.
    let seqs: Vec<i64> = sqlx::query_scalar("SELECT seq FROM changes ORDER BY seq").fetch_all(&pool).await.unwrap();
    assert_eq!(seqs, [6]);
    let next: i64 = sqlx::query_scalar(
        "INSERT INTO changes (entity, entity_uuid, op, edited_at, applied_at, user_id, device_id, client_op_id) \
         VALUES ('object_type', 't1', 'create', 't', 't', 1, 'rest', 'op-next') RETURNING seq")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(next, 8, "a seq a device may have seen is never handed out again");
    let bad = sqlx::query("INSERT INTO changes (entity, entity_uuid, op, edited_at, applied_at, user_id, device_id, client_op_id) \
         VALUES ('nonsense', 't1', 'create', 't', 't', 1, 'rest', 'op-bad')").execute(&pool).await;
    assert!(bad.is_err(), "the entity CHECK still refuses unknown entities");

    let (pool, result) = migrate_0014_over(
        "PRAGMA foreign_keys = off;
         INSERT INTO users (id, username, password_hash, is_admin, created_at) VALUES (1, 'ben', 'x', 1, 't');
         INSERT INTO objects (id, user_id, name, type, created_at, updated_at, client_uuid) VALUES (1, 1, 'Golf', 'car', 't', 't', 'u1');
         INSERT INTO activities (id, object_id, date, category, title, created_at, updated_at)
           VALUES (1, 99, '2026-02-01', 'repair', 'Orphan', 't', 't');
         PRAGMA foreign_keys = on;",
    ).await;
    let err = result.expect_err("an orphaned activity must fail the migration");
    assert!(err.contains("0014") && err.contains("CHECK"), "{err}");
    // The failure left 0014's transaction open on this one connection; a migrator that gives up
    // drops the connection, which is this rollback. Succeeding proves the COMMIT never ran, and
    // afterwards the objects table still carries the old CHECK on `type`.
    sqlx::query("ROLLBACK").execute(&pool).await.expect("0014's transaction was still open, not committed");
    let schema: String = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'objects'").fetch_one(&pool).await.unwrap();
    assert!(schema.contains("'car'"), "the rebuild must not have committed: {schema}");
}

#[tokio::test]
async fn another_users_type_cannot_be_changed_or_deleted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let created = create_scooter(&app, &app.client).await;
    let id = created["id"].as_i64().unwrap();
    let anna = app.create_user_client("anna", "password123").await;

    let res = anna.patch(app.url(&format!("/types/{id}")))
        .json(&json!({ "name": "Mine now", "icon": "box", "categories": ["repair"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 404);
    let res = anna.delete(app.url(&format!("/types/{id}"))).send().await.unwrap();
    assert_eq!(res.status(), 404);

    assert_eq!(app.get_json("/types").await, json!([created]), "ben's type is unchanged");
    let hers: Value = anna.get(app.url("/types")).send().await.unwrap().json().await.unwrap();
    assert_eq!(hers, json!([]));
}

#[tokio::test]
async fn client_uuid_replays_idempotently() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let mut body = scooter();
    body["client_uuid"] = json!("3F2504E0-4F89-11D3-9A0C-0305E82C3301");
    let res = post(&app, &app.client, "/types", body.clone()).await;
    assert_eq!(res.status(), 201);
    let first: Value = res.json().await.unwrap();
    assert_eq!(first["key"], "custom:3f2504e0-4f89-11d3-9a0c-0305e82c3301", "the uuid is stored lower case");
    let res = post(&app, &app.client, "/types", body).await;
    assert_eq!(res.status(), 200);
    let second: Value = res.json().await.unwrap();
    assert_eq!(second["id"], first["id"]);
}
