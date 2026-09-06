mod common;

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
    let pool = memto::db::connect(dir.path()).await.unwrap();
    let names: Vec<(String,)> = sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .fetch_all(&pool)
        .await
        .unwrap();
    let names: Vec<&str> = names.iter().map(|n| n.0.as_str()).collect();
    for t in ["users", "sessions", "settings", "objects", "activities", "files", "attachments", "reminders"] {
        assert!(names.contains(&t), "missing table {t}: {names:?}");
    }
    assert!(dir.path().join("memto.db").exists());
}

/// `--backup` has to produce a file a fresh instance can actually open and read.
#[tokio::test]
async fn backup_writes_a_readable_snapshot() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("snapshot.db");
    memto::db::backup_to(&app.state.db, &dest).await.unwrap();
    assert!(dest.exists());

    // The snapshot opens on its own and carries the data.
    let copied = dir.path().join("memto.db");
    std::fs::rename(&dest, &copied).unwrap();
    let pool = memto::db::connect_existing(dir.path()).await.unwrap();
    let (objects,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects").fetch_one(&pool).await.unwrap();
    let (users,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users").fetch_one(&pool).await.unwrap();
    assert_eq!((objects, users), (1, 1));
}

#[tokio::test]
async fn backup_refuses_to_overwrite_and_needs_an_existing_database() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("snapshot.db");
    memto::db::backup_to(&app.state.db, &dest).await.unwrap();
    assert!(memto::db::backup_to(&app.state.db, &dest).await.is_err(), "must not clobber an existing file");

    let empty = tempfile::tempdir().unwrap();
    assert!(memto::db::connect_existing(empty.path()).await.is_err(), "no database to back up");
}
