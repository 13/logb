//! The migration rewrites live data on an instance that has been running for weeks. What
//! matters is not that it produces the right column -- it is that nothing else moves: no
//! attachment is cascade-deleted, no reminder loses its activity, and no text a user typed is
//! thrown away.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{AssertSqlSafe, Row, SqlitePool};

/// Applies every migration up to but excluding 0009, then seeds the old shape.
async fn old_schema_with_rows() -> SqlitePool {
    let opts = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await.unwrap();
    for file in [
        "0001_init.sql", "0002_session_expiry_index.sql", "0003_fuel_quantity.sql",
        "0004_reminder_snooze.sql", "0005_client_op_id.sql", "0006_api_tokens.sql",
        "0007_sync.sql", "0008_sync_epoch.sql",
    ] {
        let sql = std::fs::read_to_string(format!("migrations/{file}")).unwrap();
        sqlx::raw_sql(AssertSqlSafe(sql)).execute(&pool).await.unwrap();
    }
    sqlx::raw_sql(
        "INSERT INTO users (id, username, password_hash, is_admin, created_at) \
         VALUES (1, 'ben', 'x', 1, '2026-01-01T00:00:00Z');
         INSERT INTO objects (id, user_id, name, category, description, created_at, updated_at) VALUES
           (1, 1, 'Golf', 'Auto', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
           (2, 1, 'Commuter', 'E-Bike', 'blue', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
           (3, 1, 'Odd one', 'Gravelbike Custom', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
           (4, 1, 'Other odd', 'Rennrad', 'already here', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
         INSERT INTO activities (id, object_id, date, category, title, created_at, updated_at) VALUES
           (1, 1, '2026-02-01', 'maintenance', 'Oil', '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z');
         INSERT INTO files (id, user_id, sha256, original_name, mime, size, created_at) \
           VALUES (1, 1, 'abc', 'a.png', 'image/png', 1, '2026-02-01T00:00:00Z');
         INSERT INTO attachments (id, object_id, activity_id, file_id, kind, caption, created_at) \
           VALUES (1, 1, 1, 1, 'photo', 'a.png', '2026-02-01T00:00:00Z');
         INSERT INTO reminders (id, object_id, title, due_date, done_activity_id, created_at) \
           VALUES (1, 1, 'Service', '2026-06-01', 1, '2026-02-01T00:00:00Z');\
         INSERT INTO field_clock (entity, entity_uuid, field, edited_at, device_id) VALUES \
           ('object', 'uuid-1', 'category', '2026-02-01T00:00:00Z', 'device-1'), \
           ('object', 'uuid-1', 'name', '2026-02-01T00:00:00Z', 'device-1');",
    ).execute(&pool).await.unwrap();
    pool
}

async fn run_0009(pool: &SqlitePool) {
    let sql = std::fs::read_to_string("migrations/0009_object_types.sql").unwrap();
    sqlx::raw_sql(AssertSqlSafe(sql)).execute(pool).await.unwrap();
}

#[tokio::test]
async fn known_categories_become_types() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let rows = sqlx::query("SELECT id, type FROM objects ORDER BY id").fetch_all(&pool).await.unwrap();
    let types: Vec<String> = rows.iter().map(|r| r.get::<String, _>("type")).collect();
    assert_eq!(types, vec!["car", "e_bike", "other", "other"]);
}

#[tokio::test]
async fn unmapped_text_is_kept_in_the_description() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let d: String = sqlx::query_scalar("SELECT description FROM objects WHERE id = 3").fetch_one(&pool).await.unwrap();
    assert_eq!(d, "Gravelbike Custom", "an empty description takes the text alone");
    let d: String = sqlx::query_scalar("SELECT description FROM objects WHERE id = 4").fetch_one(&pool).await.unwrap();
    assert_eq!(d, "already here\nRennrad", "an existing description keeps its text, on its own line");
}

/// The reason this migration is dangerous. `objects` and `activities` are both parents of
/// cascading children, so a rebuild with foreign keys enforced deletes attachments outright.
#[tokio::test]
async fn nothing_is_cascade_deleted() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let attachments: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments").fetch_one(&pool).await.unwrap();
    let activities: i64 = sqlx::query_scalar("SELECT count(*) FROM activities").fetch_one(&pool).await.unwrap();
    let objects: i64 = sqlx::query_scalar("SELECT count(*) FROM objects").fetch_one(&pool).await.unwrap();
    let done: Option<i64> = sqlx::query_scalar("SELECT done_activity_id FROM reminders WHERE id = 1").fetch_one(&pool).await.unwrap();
    assert_eq!((objects, activities, attachments), (4, 1, 1));
    assert_eq!(done, Some(1), "the reminder still points at its activity");
    let violations = sqlx::query("PRAGMA foreign_key_check").fetch_all(&pool).await.unwrap();
    assert!(violations.is_empty(), "{} foreign key violations after the rebuild", violations.len());
}

#[tokio::test]
async fn the_new_categories_are_accepted_and_nonsense_is_not() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    sqlx::query("INSERT INTO activities (object_id, date, category, title, created_at, updated_at) \
                 VALUES (1, '2026-03-01', 'symptom', 'Shoulder', 'x', 'x')")
        .execute(&pool).await.expect("symptom is a category now");
    let bad = sqlx::query("INSERT INTO activities (object_id, date, category, title, created_at, updated_at) \
                           VALUES (1, '2026-03-01', 'nonsense', 'x', 'x', 'x')")
        .execute(&pool).await;
    assert!(bad.is_err(), "the CHECK must still reject an unknown category");
}

#[tokio::test]
async fn the_indexes_survive() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND name IN \
         ('idx_objects_user', 'idx_activities_object_date') ORDER BY name")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(names, vec!["idx_activities_object_date", "idx_objects_user"]);
}

/// The three indexes the brief's SQL forgot. `client_uuid` is how a device names a row it made
/// offline and how the server recognises the same row coming back; `client_op_id` is what makes
/// a retried write idempotent. Dropping a table drops its indexes, so a rebuild that does not
/// recreate these leaves sync able to insert the same row twice with nothing complaining.
#[tokio::test]
async fn the_sync_uniqueness_indexes_survive() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND name IN \
         ('idx_objects_uuid', 'idx_activities_uuid', 'idx_activities_client_op') ORDER BY name")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(names, vec!["idx_activities_client_op", "idx_activities_uuid", "idx_objects_uuid"]);

    sqlx::query("UPDATE objects SET client_uuid = 'u1' WHERE id = 1").execute(&pool).await.unwrap();
    let dup = sqlx::query("UPDATE objects SET client_uuid = 'u1' WHERE id = 2").execute(&pool).await;
    assert!(dup.is_err(), "two objects must not share a client_uuid");

    sqlx::query("UPDATE activities SET client_op_id = 'op1' WHERE id = 1").execute(&pool).await.unwrap();
    let dup = sqlx::query("INSERT INTO activities (object_id, date, category, title, client_op_id, created_at, updated_at) \
                           VALUES (1, '2026-03-01', 'repair', 'x', 'op1', 'x', 'x')")
        .execute(&pool).await;
    assert!(dup.is_err(), "a replayed client_op_id must not create a second activity");
}

/// Devices hold a cursor plus an epoch; a mismatch sends them back to a full bootstrap. This
/// migration rewrites rows without writing anything to `changes`, so without a new epoch an
/// offline device would keep its old `category` and never learn that types exist.
#[tokio::test]
async fn the_sync_epoch_is_rotated() {
    let pool = old_schema_with_rows().await;
    let before: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'sync_epoch'").fetch_one(&pool).await.unwrap();
    run_0009(&pool).await;
    let after: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'sync_epoch'").fetch_one(&pool).await.unwrap();
    assert_ne!(before, after);
    assert_eq!(after.len(), 32);
}

/// The migration's SQL repeats the mapping that `object_type::from_legacy` holds in Rust. Two
/// copies drift. This runs every word in the Rust table through the migration's own CASE
/// expression -- as written, uppercased, and with just its first letter capitalised -- and
/// demands SQL and Rust agree on all three. The uppercased form is what catches an umlaut left
/// unfolded: SQLite's `lower()` is ASCII-only, so 'GERÄT' stays 'GERÄT' rather than becoming
/// 'gerät', while Rust's `to_lowercase()` folds it either way.
#[tokio::test]
async fn the_sql_mapping_matches_the_rust_one() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    for (word, expected) in logb::object_type::LEGACY.iter() {
        let mut chars = word.chars();
        let capitalized = match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        };
        for variant in [word.to_string(), word.to_uppercase(), capitalized] {
            let mapped: String = sqlx::query_scalar(
                "SELECT CASE REPLACE(REPLACE(lower(trim(?1)), 'Ä', 'ä'), 'Ö', 'ö') \
                   WHEN 'car' THEN 'car' WHEN 'auto' THEN 'car' WHEN 'pkw' THEN 'car' WHEN 'wagen' THEN 'car' \
                   WHEN 'e-bike' THEN 'e_bike' WHEN 'ebike' THEN 'e_bike' WHEN 'e bike' THEN 'e_bike' WHEN 'pedelec' THEN 'e_bike' \
                   WHEN 'bike' THEN 'bike' WHEN 'fahrrad' THEN 'bike' WHEN 'velo' THEN 'bike' WHEN 'rad' THEN 'bike' \
                   WHEN 'motorcycle' THEN 'motorcycle' WHEN 'motorrad' THEN 'motorcycle' WHEN 'motorbike' THEN 'motorcycle' \
                   WHEN 'home' THEN 'home' WHEN 'haus' THEN 'home' WHEN 'wohnung' THEN 'home' WHEN 'flat' THEN 'home' WHEN 'apartment' THEN 'home' \
                   WHEN 'appliance' THEN 'appliance' WHEN 'gerät' THEN 'appliance' WHEN 'geraet' THEN 'appliance' WHEN 'haushaltsgerät' THEN 'appliance' \
                   WHEN 'tool' THEN 'tool' WHEN 'werkzeug' THEN 'tool' WHEN 'maschine' THEN 'tool' \
                   WHEN 'body' THEN 'body' WHEN 'körper' THEN 'body' WHEN 'koerper' THEN 'body' WHEN 'health' THEN 'body' WHEN 'gesundheit' THEN 'body' \
                   ELSE 'other' END")
                .bind(&variant).fetch_one(&pool).await.unwrap();
            assert_eq!(&mapped, expected, "SQL and Rust disagree on {variant:?} (from {word})");
        }
    }
}

/// `DELETE FROM field_clock WHERE entity = 'object' AND field = 'category'` targets a column
/// that no longer exists. A statement that deleted the whole table would also pass a check that
/// only confirms the `category` clock is gone, so this seeds a `name` clock for the same object
/// too and demands it survives.
#[tokio::test]
async fn field_clocks_for_the_dropped_column_are_removed_and_others_survive() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let category_clock: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM field_clock WHERE entity = 'object' AND field = 'category'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(category_clock, 0, "the clock for the dropped column must be gone");
    let name_clock: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM field_clock WHERE entity = 'object' AND field = 'name'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(name_clock, 1, "a clock for a surviving column must not be swept up with it");
}

/// A rebuild that omits a column drops it from live data with nothing reporting an error. This
/// pins the full column list of both tables, in order, against what the rebuild produced.
#[tokio::test]
async fn every_column_survives_the_rebuild() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    async fn columns(pool: &SqlitePool, pragma: &'static str) -> Vec<String> {
        sqlx::query(pragma)
            .fetch_all(pool).await.unwrap()
            .iter().map(|r| r.get::<String, _>("name")).collect()
    }
    assert_eq!(
        columns(&pool, "PRAGMA table_info(objects)").await,
        vec!["id", "user_id", "name", "type", "counter_unit", "description", "purchase_date",
             "purchase_price_cents", "archived_at", "cover_attachment_id", "created_at",
             "updated_at", "fuel_unit", "client_uuid", "deleted_at"],
    );
    assert_eq!(
        columns(&pool, "PRAGMA table_info(activities)").await,
        vec!["id", "object_id", "date", "category", "title", "notes", "counter_value",
             "cost_cents", "created_at", "updated_at", "quantity_milli", "client_op_id",
             "client_uuid", "deleted_at"],
    );
}
