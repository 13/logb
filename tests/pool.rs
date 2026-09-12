//! Two of the pragmas SQLite needs cannot be expressed in a connection URL -- sqlx 0.9's URL
//! parser accepts only `mode`, `cache`, `immutable` and `vfs` -- and `AnyPool` connects by URL
//! only, so `db::connect` restores `journal_mode` (WAL) and `busy_timeout` with an after-connect
//! hook instead. Losing either is silent: without WAL every write serializes through the
//! rollback journal instead of allowing concurrent readers, and without the busy timeout a
//! writer that finds the database locked fails immediately instead of waiting.
//!
//! `foreign_keys` is asserted here too, because the app depends on `ON DELETE CASCADE` for
//! attachments and it is the one setting this hook cannot afford to lose silently. It is *not*
//! proof the hook is doing the work, though: sqlx 0.9 turns `foreign_keys` on by default for
//! SQLite regardless of this hook (disabling the hook entirely and rerunning these tests left
//! both `foreign_keys` assertions green -- only `journal_mode` went red). It is asserted
//! explicitly anyway because a driver default is not a guarantee worth relying on silently, but
//! the cascade test below passes either way and stays regardless of which mechanism enforces it.

#[tokio::test]
async fn the_after_connect_hook_runs_on_every_pooled_connection() {
    let dir = tempfile::tempdir().unwrap();
    let pool = logb::db::connect(&format!("sqlite://{}/logb.db?mode=rwc", dir.path().display()))
        .await
        .unwrap();

    // Acquire several connections without releasing any of them, so the pool has to open that
    // many distinct connections rather than handing the same one back on every sequential
    // `fetch_one`. `db::connect` caps a SQLite pool at 4, so that is exactly how many to hold.
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(pool.acquire().await.unwrap());
    }
    assert_eq!(pool.size(), 4, "expected four live connections, not the same one reused four times");

    for conn in held.iter_mut() {
        let mode: String =
            sqlx::query_scalar("PRAGMA journal_mode").fetch_one(&mut **conn).await.unwrap();
        assert_eq!(mode, "wal", "the after-connect hook did not set WAL on every connection");

        let timeout: i64 =
            sqlx::query_scalar("PRAGMA busy_timeout").fetch_one(&mut **conn).await.unwrap();
        assert_eq!(
            timeout, 5000,
            "the after-connect hook did not set the busy timeout on every connection"
        );

        let on: i64 = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(&mut **conn).await.unwrap();
        assert_eq!(on, 1, "foreign keys are off on a pooled connection");
    }
}

/// The assertion that matters, because the pragmas above are only a means to it.
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
