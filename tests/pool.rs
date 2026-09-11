//! The pragmas SQLite needs cannot be expressed in a connection URL, and `AnyPool` connects by
//! URL only. They are restored by an after-connect hook instead -- and losing them is silent:
//! nothing errors, `ON DELETE CASCADE` simply stops happening, and attachments outlive the
//! objects they belong to.

#[tokio::test]
async fn every_pooled_connection_enforces_foreign_keys() {
    let dir = tempfile::tempdir().unwrap();
    let pool = logb::db::connect(&format!("sqlite://{}/logb.db?mode=rwc", dir.path().display()))
        .await
        .unwrap();

    // Ask several times: the hook has to run for every connection the pool opens, not just the
    // first one.
    for _ in 0..4 {
        let on: i64 = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(&pool).await.unwrap();
        assert_eq!(on, 1, "foreign keys are off on a pooled connection");
    }
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode").fetch_one(&pool).await.unwrap();
    assert_eq!(mode, "wal");
}

/// The assertion that matters, because the pragma above is only a means to it.
#[tokio::test]
async fn deleting_an_object_still_takes_its_attachments() {
    let dir = tempfile::tempdir().unwrap();
    let pool = logb::db::connect(&format!("sqlite://{}/logb.db?mode=rwc", dir.path().display()))
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (id, username, password_hash, created_at) VALUES (1, 'ben', 'x', '2026-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO objects (id, user_id, name, type, description, created_at, updated_at) \
                 VALUES (1, 1, 'Golf', 'car', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO files (id, user_id, sha256, original_name, mime, size, created_at) \
                 VALUES (1, 1, 'abc', 'a.png', 'image/png', 1, '2026-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO attachments (id, object_id, file_id, kind, created_at) \
                 VALUES (1, 1, 1, 'photo', '2026-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();

    sqlx::query("DELETE FROM objects WHERE id = 1").execute(&pool).await.unwrap();

    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments").fetch_one(&pool).await.unwrap();
    assert_eq!(left, 0, "the attachment outlived its object: foreign keys are not enforced");
}
