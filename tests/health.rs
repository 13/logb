mod common;

/// Why the two `--backup` tests in this file do not run on PostgreSQL.
///
/// `db::backup_to` is `VACUUM INTO`: SQLite writing a consistent copy of itself into a second
/// file, which these tests then open as a database in its own right. PostgreSQL has no such
/// statement and no file to open, so this is a mechanism only one backend has rather than
/// behaviour that should hold on both; part five of the PostgreSQL port gives PostgreSQL a
/// backup of its own. Every other test here -- the health endpoint, and the migration check
/// that builds its own SQLite database -- runs on both.
const VACUUM_INTO_IS_SQLITE: &str =
    "`db::backup_to` is `VACUUM INTO`, a SQLite-only statement; PostgreSQL gets a backup of \
     its own in part five of the port";


#[tokio::test]
async fn health_reports_ok_and_creates_database() {
    let app = common::spawn().await;
    let res = reqwest::get(app.url("/health")).await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn migration_creates_all_tables() {
    let dir = tempfile::tempdir().unwrap();
    let pool = logb::db::connect(&logb::db::sqlite_url(dir.path()).unwrap()).await.unwrap();
    let names: Vec<(String,)> = sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .fetch_all(&pool)
        .await
        .unwrap();
    let names: Vec<&str> = names.iter().map(|n| n.0.as_str()).collect();
    for t in ["users", "sessions", "settings", "objects", "activities", "files", "attachments", "reminders"] {
        assert!(names.contains(&t), "missing table {t}: {names:?}");
    }
    assert!(dir.path().join("logb.db").exists());
}

/// `--backup` has to produce a file a fresh instance can actually open and read.
#[tokio::test]
async fn backup_writes_a_readable_snapshot() {
    if common::skipped_on_postgres("backup_writes_a_readable_snapshot", VACUUM_INTO_IS_SQLITE) {
        return;
    }
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("snapshot.db");
    logb::db::backup_to(&app.state.db, &dest).await.unwrap();
    assert!(dest.exists());

    // The snapshot opens on its own and carries the data.
    let copied = dir.path().join("logb.db");
    std::fs::rename(&dest, &copied).unwrap();
    let pool = logb::db::connect_existing(&logb::db::sqlite_url(dir.path()).unwrap()).await.unwrap();
    let (objects,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects").fetch_one(&pool).await.unwrap();
    let (users,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users").fetch_one(&pool).await.unwrap();
    assert_eq!((objects, users), (1, 1));
}

#[tokio::test]
async fn backup_refuses_to_overwrite_and_needs_an_existing_database() {
    if common::skipped_on_postgres("backup_refuses_to_overwrite_and_needs_an_existing_database", VACUUM_INTO_IS_SQLITE) {
        return;
    }
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("snapshot.db");
    logb::db::backup_to(&app.state.db, &dest).await.unwrap();
    assert!(logb::db::backup_to(&app.state.db, &dest).await.is_err(), "must not clobber an existing file");

    let empty = tempfile::tempdir().unwrap();
    assert!(logb::db::connect_existing(&logb::db::sqlite_url(empty.path()).unwrap()).await.is_err(), "no database to back up");
}

#[tokio::test]
async fn health_reports_the_database_state() {
    let app = common::spawn().await;
    let res = app.client.get(app.url("/health")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body["migrations"].as_i64().unwrap() > 0, "it says how much schema it found");
}

#[tokio::test]
async fn health_turns_503_when_the_schema_is_incomplete() {
    let app = common::spawn().await;
    // Exactly the shape of the real incident: the process is up and serving, but the database
    // under it is not the one the binary was built for.
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = (SELECT max(version) FROM _sqlx_migrations)")
        .execute(&app.state.db).await.unwrap();

    let res = app.client.get(app.url("/health")).send().await.unwrap();
    assert_eq!(res.status(), 503, "a liveness probe must fail when the schema is behind");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "unavailable");
}

#[tokio::test]
async fn health_turns_503_when_the_database_is_unreachable() {
    let app = common::spawn().await;
    // The query-failure path: drop the entire migrations table so the query fails,
    // not just returns a low count. This is different from the row-count path above.
    sqlx::query("DROP TABLE _sqlx_migrations")
        .execute(&app.state.db).await.unwrap();

    let res = app.client.get(app.url("/health")).send().await.unwrap();
    assert_eq!(res.status(), 503, "health must fail when the database query fails");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "unavailable");
}
