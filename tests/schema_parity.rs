//! The two schemas are written by hand, in different dialects, and nothing but this test makes
//! them agree. A column added to one and forgotten in the other does not fail at compile time:
//! it fails at runtime, on whichever instance happens to be running the other backend.

use std::collections::BTreeSet;

/// Every `table.column` in the database, as the database itself reports it.
///
/// Read from the catalogue rather than from the migration files, so it describes what the
/// migrations actually produced -- including the columns SQLite's `ALTER TABLE ADD COLUMN`
/// steps bolted on later, which no single file in `migrations/sqlite` shows.
async fn columns(url: &str) -> BTreeSet<String> {
    let pool = logb::db::connect(url).await.unwrap();
    let sql = if url.starts_with("sqlite:") {
        // `sqlite_%` is SQLite's own bookkeeping (`sqlite_sequence`, from AUTOINCREMENT), and
        // `_sqlx_migrations` is the migrator's -- neither is part of the schema under test,
        // and neither has a PostgreSQL counterpart of the same shape.
        "SELECT m.name || '.' || p.name FROM sqlite_master m \
         JOIN pragma_table_info(m.name) p WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' \
         AND m.name <> '_sqlx_migrations'"
    } else {
        "SELECT table_name || '.' || column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations'"
    };
    let found: Vec<String> = sqlx::query_scalar(sql).fetch_all(&pool).await.unwrap();
    pool.close().await;
    assert!(!found.is_empty(), "{url} reported no columns at all -- the migrations did not run");
    found.into_iter().collect()
}

#[tokio::test]
async fn the_two_schemas_describe_the_same_tables_and_columns() {
    // `LOGB_TEST_DATABASE_URL` is the test harness's own variable, separate from the app's
    // `LOGB_DATABASE_URL`: it names a scratch PostgreSQL server this test may migrate into,
    // not the database an instance serves. A skipped test is not a passing test -- CI always
    // provides the URL, so this cannot be quietly skipped forever.
    let Ok(pg) = std::env::var("LOGB_TEST_DATABASE_URL") else {
        eprintln!("skipped: set LOGB_TEST_DATABASE_URL to a PostgreSQL server to run this");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sqlite = columns(&format!("sqlite://{}/logb.db?mode=rwc", dir.path().display())).await;
    let postgres = columns(&pg).await;

    let only_sqlite: Vec<_> = sqlite.difference(&postgres).collect();
    let only_postgres: Vec<_> = postgres.difference(&sqlite).collect();
    assert!(
        only_sqlite.is_empty() && only_postgres.is_empty(),
        "schemas disagree.\n  only in SQLite:     {only_sqlite:?}\
         \n  only in PostgreSQL: {only_postgres:?}"
    );
}
