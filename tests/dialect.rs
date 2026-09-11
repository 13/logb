//! Searching and sorting are where the two databases disagree most quietly. SQLite's `LIKE` is
//! case-insensitive for ASCII only; PostgreSQL's `LIKE` is not case-insensitive at all and needs
//! `ILIKE`, which folds by the server's collation. A German user searching "ölwechsel" for an
//! entry titled "Ölwechsel" is the case that decides whether this app behaves the same on both.
//!
//! These run against whichever backend the harness is configured for, so they catch a dialect
//! difference rather than describing one. The one place the two genuinely cannot be made to
//! agree is pinned per backend below, so that it stays a known difference instead of a surprise.

mod common;

use logb::dialect::Backend;

fn hits(results: &serde_json::Value, kind: &str) -> usize {
    results[kind].as_array().unwrap_or_else(|| panic!("no {kind} in {results}")).len()
}

/// Everything both backends must agree on: case is irrelevant to ASCII letters, in the term
/// and in the stored text alike, and an accented character matches itself.
#[tokio::test]
async fn search_ignores_case_on_both_backends() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    app.create_activity(&object["id"], "Ölwechsel").await;

    for term in ["Ölwechsel", "ÖLWECHSEL", "wechsel", "WECHSEL", "Ölwech", "golf", "GOLF"] {
        let results = app.search(term).await;
        let found = hits(&results, "activities") + hits(&results, "objects");
        assert_eq!(found, 1, "searching {term} found nothing: {results}");
    }
}

/// The one difference the two backends cannot be talked out of.
///
/// SQLite's built-in `LIKE` folds the 26 ASCII letters and nothing else, so a lowercase "ö" in
/// the term does not match the stored "Ö". PostgreSQL's `ILIKE` folds by the database
/// collation, so on a UTF-8 server it does. Closing the gap would mean an ICU build of SQLite
/// or a second, case-folded copy of every searchable column; neither is worth it for a search
/// box over one household's belongings.
///
/// Asserted rather than skipped, in both directions: if SQLite ever starts folding accents, or
/// PostgreSQL ever stops, this is the test that says so.
#[tokio::test]
async fn only_postgresql_folds_the_case_of_accented_letters() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    app.create_activity(&object["id"], "Ölwechsel").await;

    let expected = match app.state.backend {
        // A property of SQLite's built-in `LIKE`, true of every build without ICU.
        Backend::Sqlite => 0,
        // PostgreSQL's answer depends on the cluster's collation -- a `C`-locale cluster folds
        // no more than SQLite does -- so ask the server what its own operator says and require
        // the endpoint to agree with it. On the ordinary UTF-8 cluster this is 1, and a search
        // still built on `LIKE` would answer 0 and fail here.
        Backend::Postgres => {
            let folds: logb::db::Bool = sqlx::query_scalar("SELECT 'Ölwechsel' ILIKE '%ölwech%'")
                .fetch_one(&app.state.db).await.unwrap();
            usize::from(folds.0)
        },
    };
    for term in ["ölwechsel", "ölwech"] {
        let results = app.search(term).await;
        assert_eq!(
            hits(&results, "activities"),
            expected,
            "{:?} searching {term} for \"Ölwechsel\": {results}",
            app.state.backend
        );
    }
}

/// SQLite gets this from `UNIQUE COLLATE NOCASE` on the column; PostgreSQL from a unique index
/// on `lower(username)`. Both only work if every lookup compares the same way.
#[tokio::test]
async fn usernames_are_case_insensitive_for_uniqueness_and_sign_in() {
    let app = common::spawn().await;
    app.setup("Ben", "correct horse").await;
    let taken = app.create_user("BEN", "another one").await;
    assert_eq!(taken.status(), 409, "username uniqueness must ignore case on both backends");
    assert_eq!(app.sign_in("bEn", "correct horse").await.status(), 200);
}

#[tokio::test]
async fn objects_sort_without_regard_to_case() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for name in ["apple", "Banana", "cherry"] {
        app.create_object(&app.client, name, None).await;
    }
    let names = app.object_names().await;
    assert_eq!(names, vec!["apple", "Banana", "cherry"], "case must not push Banana to the front");
}

/// Archived objects are listed by their own query, and sort the same way.
#[tokio::test]
async fn archived_objects_sort_without_regard_to_case_too() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for name in ["apple", "Banana", "cherry"] {
        let object = app.create_object(&app.client, name, None).await;
        let id = object["id"].as_i64().unwrap();
        let res = app.client.patch(app.url(&format!("/objects/{id}")))
            .json(&serde_json::json!({
                "name": name, "type": "car", "counter_unit": null, "description": "",
                "purchase_date": null, "purchase_price_cents": null, "archived": true
            }))
            .send().await.unwrap();
        assert_eq!(res.status(), 200, "archive failed: {}", res.text().await.unwrap());
    }
    let res = app.client.get(app.url("/objects?archived=true")).send().await.unwrap();
    let rows: serde_json::Value = res.json().await.unwrap();
    let names: Vec<&str> = rows.as_array().unwrap().iter().map(|o| o["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["apple", "Banana", "cherry"], "{rows}");
}

/// The SQLite migrations seeded `currency` and `sync_epoch`; the PostgreSQL schema seeds
/// nothing, because the app now writes both on first start on either backend. Without the
/// epoch row every sync pull and push is a 500, so this is the row a fresh PostgreSQL database
/// most needs.
///
/// Deleting the rows and reconnecting exercises the seeding on the backend under test, rather
/// than only observing what the SQLite migrations happened to leave behind.
#[tokio::test]
async fn a_start_seeds_the_settings_rows_the_app_cannot_run_without() {
    let app = common::spawn().await;
    let url = app.state.config.database_url().unwrap();

    sqlx::query("DELETE FROM settings WHERE key IN ('sync_epoch', 'currency')")
        .execute(&app.state.db).await.unwrap();

    let pool = logb::db::connect(&url).await.expect("a second start over the same database");
    let epoch = logb::sync::epoch::current(&pool).await.expect("the epoch row is seeded, not missing");
    assert_eq!(epoch.len(), 32, "16 random bytes as hex, the shape the migrations produced: {epoch}");
    let currency: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'currency'")
        .fetch_one(&pool).await.expect("the currency row is seeded too");
    assert_eq!(currency, "EUR");
    pool.close().await;
}

/// Seeding runs on every start, so it has to be idempotent -- and specifically it must not mint
/// a new epoch over an existing one. That would change the database's identity behind every
/// device's back and send them all on a full re-bootstrap, on a restart that changed nothing.
#[tokio::test]
async fn a_later_start_does_not_rotate_the_epoch_or_reset_the_currency() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let before = logb::sync::epoch::current(&app.state.db).await.unwrap();

    let res = app.client.put(app.url("/settings")).json(&serde_json::json!({ "currency": "CHF" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let url = app.state.config.database_url().unwrap();
    let pool = logb::db::connect(&url).await.unwrap();
    assert_eq!(
        logb::sync::epoch::current(&pool).await.unwrap(), before,
        "a restart must not give the database a new identity"
    );
    let currency: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'currency'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(currency, "CHF", "a restart must not overwrite a currency the operator chose");
    pool.close().await;
}
