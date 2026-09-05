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
