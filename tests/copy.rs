//! Moving a database to another backend. The thing that matters is that identifiers survive:
//! every foreign key still points where it did, and every device's stored ids still resolve.

mod common;

/// Seeds a database through the real API, so the copy is exercised against rows the application
/// actually produces rather than rows a test invented.
async fn seeded() -> common::TestApp {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    app.create_activity(&object["id"], "Ölwechsel").await;
    app.create_activity(&object["id"], "Winter tyres").await;
    app
}

#[tokio::test]
async fn every_row_and_identifier_survives_the_copy() {
    let app = seeded().await;
    // A copy refuses a source something else still holds -- see
    // `copying_from_a_database_still_in_use_is_refused` -- so the app lets go of it first,
    // which is what an operator stopping the server does.
    app.release_database().await;
    let dest = common::scratch_database().await;

    let report = logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap();

    // Row counts, per table, in the order they were copied.
    let copied: Vec<(String, i64)> = report.tables.clone();
    assert!(copied.iter().any(|(t, n)| t == "objects" && *n == 1), "{copied:?}");
    assert!(copied.iter().any(|(t, n)| t == "activities" && *n == 2), "{copied:?}");

    // Identifiers, not just counts: the activities must still hang off the same object id.
    let src_rows = app.all_activity_ids().await;
    let dest_rows = dest.all_activity_ids().await;
    assert_eq!(src_rows, dest_rows, "activity ids or their object_id changed in the copy");
}

#[tokio::test]
async fn a_destination_that_already_holds_data_is_refused() {
    let app = seeded().await;
    app.release_database().await;
    let dest = common::scratch_database().await;
    logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap();

    // Copying again without --force would merge two histories into one database.
    let err = logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap_err().to_string();
    assert!(err.contains("--force"), "the refusal must name the way round it: {err}");
}

/// Copying out of a database a server is still writing to would capture a moving target: later
/// tables read after earlier ones changed, internally inconsistent, reported as success.
#[tokio::test]
async fn copying_from_a_database_still_in_use_is_refused() {
    let app = seeded().await;              // its server is running and holds the database
    let dest = common::scratch_database().await;
    let err = logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap_err().to_string();
    assert!(err.contains("stop the server"), "the refusal must say what to do: {err}");
}

/// A copy nobody can write to afterwards is not a copy of a working database.
///
/// PostgreSQL's identity columns hand out ids from a sequence that an explicitly written `id`
/// does not advance, so a copy that left the sequences alone would hand the very first row the
/// new database wrote an id it had just been given -- a duplicate key on the first object
/// anyone created, against a database that reported a clean copy.
#[tokio::test]
async fn the_copy_can_still_take_new_rows_of_its_own() {
    let app = seeded().await;
    app.release_database().await;
    let dest = common::scratch_database().await;
    logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap();

    let pool = logb::db::connect_existing(&dest.url).await.unwrap();
    // No `id`: the database picks one, exactly as every insert in the app does.
    sqlx::query(
        "INSERT INTO users (username, password_hash, created_at) \
         VALUES ('later', 'x', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .expect("the copied database must be able to write a row of its own");
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM users ORDER BY id").fetch_all(&pool).await.unwrap();
    pool.close().await;
    assert_eq!(ids.len(), 2, "the copied user and the new one: {ids:?}");
}
