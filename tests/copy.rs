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

    let report = logb::copy::run(&app.database_url(), &dest.url).await.unwrap();

    // Row counts, per table, in the order they were copied.
    let copied: Vec<(String, i64)> = report.tables.clone();
    assert!(copied.iter().any(|(t, n)| t == "objects" && *n == 1), "{copied:?}");
    assert!(copied.iter().any(|(t, n)| t == "activities" && *n == 2), "{copied:?}");

    // Identifiers, not just counts: the activities must still hang off the same object id.
    let src_rows = app.all_activity_ids().await;
    let dest_rows = dest.all_activity_ids().await;
    assert_eq!(src_rows, dest_rows, "activity ids or their object_id changed in the copy");
}

/// A child that was created before its parent still crosses.
///
/// `objects.parent_id` is the only foreign key in the schema that points at its own table, and
/// it is not `DEFERRABLE`: both backends check it as each row is written, not at commit. The
/// rows come out of the source in creation order, and creation order says nothing about tree
/// order -- buy a bike, build a garage a year later, put the bike in the garage, and the child
/// holds the lower id. Written in that order the child names a parent the destination has not
/// been given yet, and the copy aborts partway through with a foreign key violation.
///
/// The seeding is deliberately the shape `seeded()` cannot produce: its objects are all roots,
/// so it would pass against a copy that wrote them in any order at all.
#[tokio::test]
async fn a_child_created_before_its_parent_still_copies() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    // The child first, so the database hands it the lower id.
    let bike = app.create_object(&app.client, "Bike", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let (bike, garage) = (bike["id"].as_i64().unwrap(), garage["id"].as_i64().unwrap());
    assert!(bike < garage, "the child must have been created first: {bike} and {garage}");

    // Moved in through the real API, so the row is one the application actually produces.
    let res = app
        .client
        .patch(app.url(&format!("/objects/{bike}")))
        .json(&serde_json::json!({ "name": "Bike", "type": "bike", "parent_id": garage }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "reparenting failed: {}", res.text().await.unwrap());

    // As every copy test does: the source has to be let go of before it can be copied out of.
    app.release_database().await;
    let dest = common::scratch_database().await;

    let report = logb::copy::run(&app.database_url(), &dest.url).await.unwrap();
    assert!(report.tables.iter().any(|(t, n)| t == "objects" && *n == 2), "{:?}", report.tables);

    // Not merely "it did not fail": both rows are there, with the same ids, and the bike is
    // still inside the garage.
    let pool = logb::db::connect_existing(&dest.url).await.unwrap();
    let rows: Vec<(i64, String, Option<i64>)> =
        sqlx::query_as("SELECT id, name, parent_id FROM objects ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    pool.close().await;
    assert_eq!(
        rows,
        vec![(bike, "Bike".to_string(), Some(garage)), (garage, "Garage".to_string(), None)],
        "the copied tree must hold the same ids and the same parent link"
    );
}

#[tokio::test]
async fn a_destination_that_already_holds_data_is_refused() {
    let app = seeded().await;
    app.release_database().await;
    let dest = common::scratch_database().await;
    logb::copy::run(&app.database_url(), &dest.url).await.unwrap();

    // Copying again would merge two histories into one database. There is no flag to override
    // it: both databases number their rows from 1, so the second copy collides on the first
    // primary key it writes. The refusal has to say why.
    let err = logb::copy::run(&app.database_url(), &dest.url).await.unwrap_err().to_string();
    assert!(err.contains("two histories in one database"), "the refusal must say why: {err}");
}

/// Copying out of a database a server is still writing to would capture a moving target: later
/// tables read after earlier ones changed, internally inconsistent, reported as success.
#[tokio::test]
async fn copying_from_a_database_still_in_use_is_refused() {
    let app = seeded().await;              // its server is running and holds the database
    let dest = common::scratch_database().await;
    let err = logb::copy::run(&app.database_url(), &dest.url).await.unwrap_err().to_string();
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
    logb::copy::run(&app.database_url(), &dest.url).await.unwrap();

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

/// A copy that reports success while having dropped rows is worse than one that fails: the
/// operator deletes the source. So the command counts both sides and compares, and a mismatch
/// is an error naming the table.
#[tokio::test]
async fn a_copy_that_loses_rows_fails_and_names_the_table() {
    let app = seeded().await;
    // As every copy test does: the source has to be let go of before it can be copied out of.
    app.release_database().await;
    let dest = common::scratch_database().await;

    // Delete a row from the destination mid-copy by racing is unreliable; instead copy, then
    // remove a row and re-run the verification directly. That is the same check the command
    // performs, against a destination that is genuinely wrong.
    logb::copy::run(&app.database_url(), &dest.url).await.unwrap();
    dest.delete_one_activity().await;

    let err = logb::copy::verify(&app.database_url(), &dest.url).await.unwrap_err().to_string();
    assert!(err.contains("activities"), "the error must name the table that differs: {err}");
}

/// Running the verification before the commit is the point of it: a copy that did not arrive
/// has to leave nothing behind at all, rather than a half-copied database that looks finished.
#[tokio::test]
async fn a_copy_that_fails_verification_commits_nothing() {
    let app = seeded().await;
    app.release_database().await;
    let dest = common::scratch_database().await;
    dest.plant_a_stray_row().await;

    let err = logb::copy::run(&app.database_url(), &dest.url).await.unwrap_err().to_string();
    assert!(err.contains("field_clock"), "the error must name the table that differs: {err}");
    assert_eq!(dest.user_count().await, 0, "a failed verification must leave nothing committed");
}

/// Every table with a stable order to read it in, so two databases can be compared row for row.
/// `settings` is deliberately absent: the copy rotates the `sync_epoch` in it on purpose, and
/// it is checked on its own below.
const COMPARABLE: [(&str, &str); 10] = [
    ("users", "id"),
    ("api_tokens", "id"),
    ("sessions", "token"),
    ("objects", "id"),
    ("activities", "id"),
    ("files", "id"),
    ("attachments", "id"),
    ("reminders", "id"),
    ("changes", "seq"),
    ("field_clock", "entity, entity_uuid, field"),
];

/// SQLite to PostgreSQL: the copy this command exists to perform, and the one no ordinary suite
/// run reaches -- every other test in this file copies a database into another of its own kind,
/// because a run is against one backend at a time. Skipped unless a PostgreSQL server is
/// configured, because there is nothing to copy into without one.
///
/// What only a change of engine can break is types. SQLite keeps a type per value and hands
/// back whatever it was given; PostgreSQL holds every value to its column's declared type, and
/// the two schemas do not declare the same shapes -- `is_admin` is INTEGER on one side and
/// SMALLINT on the other, timestamps are TEXT holding ISO-8601 rather than a timestamp type,
/// an absent value is NULL where an empty one is `''`, and a log that has been running for
/// years carries a `seq` past what 32 bits hold. So the seeding below produces all of those
/// through the real API, and the comparison is value by value: the copy's own verification is a
/// fingerprint -- count, key sum, newest `updated_at` -- and would not notice a changed value.
#[tokio::test]
async fn a_sqlite_database_copies_into_postgresql() {
    let Some(server) = common::test_server_url() else {
        eprintln!(
            "SKIPPED: a_sqlite_database_copies_into_postgresql -- \
             set LOGB_TEST_DATABASE_URL to a PostgreSQL server to run it"
        );
        return;
    };
    if logb::dialect::Backend::of(&server) != logb::dialect::Backend::Postgres {
        eprintln!("SKIPPED: a_sqlite_database_copies_into_postgresql -- {server} is not PostgreSQL");
        return;
    }

    // A SQLite source, deliberately, whatever backend the rest of the suite is running on.
    let dir = tempfile::tempdir().unwrap();
    let source = logb::db::sqlite_url(dir.path()).unwrap();
    let app = common::spawn_on(&source).await;
    let object_id = seed_every_awkward_shape(&app).await;
    // As every copy test does: a running server holds the source, and the copy refuses it.
    app.release_database().await;

    let (_database, dest_url) = common::scratch_database_on(&server).await;
    let report = logb::copy::run(&source, &dest_url).await.unwrap();
    assert!(report.tables.iter().any(|(t, n)| t == "users" && *n == 2), "{:?}", report.tables);
    assert!(report.tables.iter().any(|(t, n)| t == "activities" && *n == 2), "{:?}", report.tables);
    assert!(report.tables.iter().any(|(t, n)| t == "attachments" && *n == 2), "{:?}", report.tables);

    // The comparison below is only worth what the seeding put in front of it: a value-by-value
    // assertion over rows that all turned out to be NULL would pass for the wrong reason.
    the_awkward_shapes_are_really_in_the_source(&source).await;

    // Content, not counts: every value of every row, on both sides.
    for (table, order) in COMPARABLE {
        let before = dump(&source, table, order).await;
        let after = dump(&dest_url, table, order).await;
        assert!(!before.is_empty(), "{table} was never seeded, so copying it proves nothing");
        assert_eq!(before, after, "{table} does not hold the same values after the copy");
    }

    // `settings` is the one table the copy is meant to change: the epoch is rotated, so every
    // device re-bootstraps rather than resuming a cursor against a database it has not seen.
    let before = settings(&source).await;
    let after = settings(&dest_url).await;
    assert_eq!(value(&before, "currency"), value(&after, "currency"), "the currency must survive");
    assert_eq!(value(&after, "sync_epoch"), report.epoch, "the destination must advertise the reported epoch");
    assert_ne!(value(&before, "sync_epoch"), value(&after, "sync_epoch"), "the epoch must be rotated");

    // And the copy is usable, rather than merely correct: an app starts on it and serves the
    // data. The login is part of the assertion -- the password hash crossed as text, and a
    // database nobody can log into has not been migrated.
    let moved = common::spawn_on(&dest_url).await;
    let res = moved.login(&moved.client, "ben", "correct horse").await;
    assert_eq!(res.status(), 200, "the copied password must still authenticate: {}", res.text().await.unwrap());

    let objects = moved.get_json("/objects").await;
    assert_eq!(objects[0]["name"], "Golf");
    assert_eq!(objects[0]["type"], "car");
    assert_eq!(objects[0]["counter_unit"], "km");
    assert_eq!(objects[0]["description"], "", "an empty description must not come back as null");

    let activities = moved.get_json(&format!("/objects/{object_id}/activities")).await;
    let titles: Vec<&str> = activities.as_array().unwrap().iter().map(|a| a["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"Ölwechsel"), "non-ASCII text must arrive intact: {titles:?}");
    let fuel = activities.as_array().unwrap().iter().find(|a| a["category"] == "fuel").expect("the fuel activity");
    assert_eq!(fuel["quantity_milli"], 38_500);
    assert_eq!(fuel["cost_cents"], 7_250);
    assert_eq!(fuel["counter_value"], 123_456);
    assert_eq!(
        fuel["attachments"][0]["original_name"], "Rechnung für Öl.txt",
        "a non-ASCII file name must survive the crossing too: {fuel}"
    );

    // The sync log is what a device resumes against, and its cursor is the value most likely to
    // be quietly narrowed on the way across.
    let seqs = change_seqs(&dest_url).await;
    assert!(seqs.iter().all(|seq| *seq > i64::from(i32::MAX)), "a large seq must cross whole: {seqs:?}");
    assert_eq!(seqs, change_seqs(&source).await, "the cursor numbers must be the ones the source had");
}

/// Seeds, through the real API, every shape of value a change of engine could plausibly mangle,
/// and answers the id of the object most of it hangs off.
async fn seed_every_awkward_shape(app: &common::TestApp) -> i64 {
    // The admin, whose `is_admin` is 1 against an INTEGER column on one side and a SMALLINT on
    // the other, and a second user whose is 0 -- so a copy that lost the column would still
    // have to explain the admin.
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;

    // `description` is '' and `purchase_date` is NULL in the same row: an empty value and an
    // absent one, which a row count cannot tell apart and PostgreSQL will not confuse.
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = object["id"].as_i64().unwrap();
    // A second user's object, with no counter at all, so `counter_unit` is NULL somewhere.
    app.create_object(&anna, "Fahrrad", None).await;

    // A child created *before* its parent, which is the ordinary shape of a tree built up over
    // time: an object bought first and moved into something bought later. `objects.parent_id`
    // is the schema's one self-referencing foreign key and PostgreSQL checks it as each row is
    // written, so a copy that wrote these two in the order they were created would name a
    // parent the destination has not been given yet and abort. See
    // `a_child_created_before_its_parent_still_copies`, which asserts the property directly;
    // this is the same shape crossing engines.
    let trailer = app.create_object(&app.client, "Anhänger", None).await;
    let shed = app.create_object(&app.client, "Scheune", None).await;
    let res = app
        .client
        .patch(app.url(&format!("/objects/{}", trailer["id"])))
        .json(&serde_json::json!({ "name": "Anhänger", "type": "other", "parent_id": shed["id"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "reparenting failed: {}", res.text().await.unwrap());

    app.create_activity(&object["id"], "Ölwechsel").await;
    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&serde_json::json!({
            "date": "2024-04-01", "category": "fuel", "title": "Tanken – Süd",
            "notes": "38,5 l für 72,50 €",
            "counter_value": 123_456, "cost_cents": 7_250, "quantity_milli": 38_500
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "create fuel activity failed: {}", res.text().await.unwrap());
    let fuel: serde_json::Value = res.json().await.unwrap();

    // A reminder with a due date and no counter: half its columns are NULL, and the CHECK on
    // the destination refuses a row where both are.
    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/reminders")))
        .json(&serde_json::json!({ "title": "TÜV", "due_date": "2030-01-01" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "create reminder failed: {}", res.text().await.unwrap());

    // Two uploads: one on the activity, one on the object alone, so `attachments.activity_id`
    // is a number in one row and NULL in the other. A text file has no dimensions, so
    // `files.width` and `files.height` are NULL too, beside a `size` that is not.
    let base = app.url(&format!("/objects/{id}/attachments"));
    for (bytes, name, activity) in [
        (b"rechnung".to_vec(), "Rechnung für Öl.txt", Some(fuel["id"].as_i64().unwrap())),
        (b"handbuch".to_vec(), "Handbuch.txt", None),
    ] {
        let mut form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(bytes)
                .file_name(name.to_string())
                .mime_str("text/plain")
                .unwrap(),
        );
        if let Some(activity) = activity {
            form = form.text("activity_id", activity.to_string());
        }
        let res = app.client.post(&base).multipart(form).send().await.unwrap();
        assert_eq!(res.status(), 201, "upload failed: {}", res.text().await.unwrap());
    }

    // Two API tokens, one of them used: `last_used_at` is a NULL in one row and an ISO-8601
    // timestamp in a TEXT column in the other.
    let mut used = String::new();
    for name in ["phone", "laptop"] {
        let res = app
            .client
            .post(app.url("/auth/tokens"))
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201, "create token failed: {}", res.text().await.unwrap());
        let body: serde_json::Value = res.json().await.unwrap();
        used = body["token"].as_str().unwrap().to_string();
    }
    let bare = reqwest::Client::new();
    let res = bare.get(app.url("/objects")).bearer_auth(&used).send().await.unwrap();
    assert_eq!(res.status(), 200, "the token should work: {}", res.text().await.unwrap());

    // A pull cursor past what 32 bits hold, which is what a database that has been logging for
    // years arrives with. The API cannot produce one -- the log numbers its own rows -- so it
    // is moved here, before the server lets go of the database.
    sqlx::query("UPDATE changes SET seq = seq + 4294967296").execute(&app.state.db).await.unwrap();

    id
}

/// Checks that the source really holds each shape this test claims to carry across.
///
/// Written against the rendered rows rather than against the API's answers so that it asks the
/// same question the comparison does: what is in the database, spelled the way the comparison
/// spells it.
async fn the_awkward_shapes_are_really_in_the_source(source: &str) {
    for (table, shape) in [
        // An INTEGER column on this side, a SMALLINT on the other, in both of its states.
        ("users", "is_admin=1"),
        ("users", "is_admin=0"),
        // An empty value and an absent one in the same row, which are not the same thing.
        ("objects", "description=\"\""),
        ("objects", "purchase_date=NULL"),
        ("objects", "counter_unit=NULL"),
        // A child whose parent was created after it, so the parent holds the higher id.
        ("objects", "id=3 name=\"Anhänger\" parent_id=4"),
        // Non-ASCII text, and numbers that are not ids.
        ("activities", "title=\"Ölwechsel\""),
        ("activities", "quantity_milli=38500"),
        // An ISO-8601 timestamp in a TEXT column, and the same column NULL in another row.
        ("api_tokens", "last_used_at=NULL"),
        ("api_tokens", "last_used_at=\"20"),
        // A nullable foreign key, in both of its states.
        ("attachments", "activity_id=NULL"),
        ("attachments", "activity_id=2"),
        ("files", "width=NULL"),
        ("reminders", "due_counter=NULL"),
    ] {
        let rows = dump(source, table, "1").await;
        assert!(
            rows.iter().any(|row| {
                // Columns are sorted by name in `dump`, but a schema change may insert another
                // column between the fields being checked. Match the required cells rather than
                // relying on them remaining adjacent in the rendered row.
                shape.split_whitespace().all(|part| row.split_whitespace().any(|cell| cell == part))
            }),
            "the seeding was meant to put {shape} in {table}, and did not: {rows:?}"
        );
    }
}

/// Every row of one table as `column=value` text, in a stable order.
///
/// Rendered as text on purpose: the question is whether the *content* crossed, and the two
/// backends will not agree on the type of a value that did -- SQLite answers with what it
/// stored, PostgreSQL with what the column declares. Sorted by column name because the two
/// schemas are guaranteed to have the same columns, not to list them in the same order.
async fn dump(url: &str, table: &str, order: &str) -> Vec<String> {
    use sqlx::any::AnyTypeInfoKind;
    use sqlx::{Column, Row, ValueRef};

    let pool = logb::db::connect_existing(url).await.unwrap();
    let sql = format!("SELECT * FROM {table} ORDER BY {order}");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_all(&pool).await.unwrap();
    pool.close().await;
    rows.iter()
        .map(|row| {
            let mut cells: Vec<String> = row
                .columns()
                .iter()
                .map(|column| {
                    let i = column.ordinal();
                    let value = match row.try_get_raw(i).unwrap().type_info().kind() {
                        AnyTypeInfoKind::Null => "NULL".to_string(),
                        AnyTypeInfoKind::Bool => row.get::<bool, _>(i).to_string(),
                        AnyTypeInfoKind::SmallInt => row.get::<i16, _>(i).to_string(),
                        AnyTypeInfoKind::Integer => row.get::<i32, _>(i).to_string(),
                        AnyTypeInfoKind::BigInt => row.get::<i64, _>(i).to_string(),
                        AnyTypeInfoKind::Real => row.get::<f32, _>(i).to_string(),
                        AnyTypeInfoKind::Double => row.get::<f64, _>(i).to_string(),
                        AnyTypeInfoKind::Text => format!("{:?}", row.get::<String, _>(i)),
                        AnyTypeInfoKind::Blob => format!("{:?}", row.get::<Vec<u8>, _>(i)),
                    };
                    format!("{}={value}", column.name())
                })
                .collect();
            cells.sort();
            cells.join(" ")
        })
        .collect()
}

/// The `settings` table as pairs, which is the one table the copy deliberately changes.
async fn settings(url: &str) -> Vec<(String, String)> {
    let pool = logb::db::connect_existing(url).await.unwrap();
    let rows = sqlx::query_as::<_, (String, String)>("SELECT key, value FROM settings ORDER BY key")
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    rows
}

fn value<'a>(settings: &'a [(String, String)], key: &str) -> &'a str {
    settings.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or_else(|| panic!("no {key} setting"))
}

/// Every `seq` in the change log, in order -- the numbers a device's cursor is measured against.
async fn change_seqs(url: &str) -> Vec<i64> {
    let pool = logb::db::connect_existing(url).await.unwrap();
    let seqs = sqlx::query_scalar::<_, i64>("SELECT seq FROM changes ORDER BY seq").fetch_all(&pool).await.unwrap();
    pool.close().await;
    seqs
}

/// The Settings flow copies from the database the server is using, which `run` deliberately
/// refuses. `run_live` does it from the pool the server already holds, inside the write
/// transaction -- so the snapshot is consistent and no write can land on the old database while
/// the copy is in flight.
#[tokio::test]
async fn a_running_server_can_copy_its_own_database() {
    let app = seeded().await;                 // its server is running and holds the database
    let dest = common::scratch_database().await;

    let report = logb::copy::run_live(&app.state.db, app.state.backend, &dest.url).await.unwrap();

    assert!(report.tables.iter().any(|(t, n)| t == "activities" && *n == 2), "{:?}", report.tables);
    // And the source is untouched and still serving: this is not a move. `/objects` answers a
    // bare array -- indexing it by a key would be `Null` whatever the copy did, which is no
    // assertion at all.
    let objects: serde_json::Value = app.get_json("/objects").await;
    assert_eq!(objects[0]["name"], "Golf");
}

/// A source database written before `0010_object_hierarchy.sql` still copies.
///
/// `run` opens the source with `db::connect_existing`, which deliberately does not migrate it,
/// so the objects table of an old backup -- or of a previous release's database being copied by
/// a new binary -- simply has no `parent_id` column. `copy_table` has always coped, because it
/// takes its column list from the rows it actually read; `parents_before_children` did not, and
/// asked every row for a column that was not there. The copy then aborted before writing
/// anything, with a raw `ColumnNotFound("parent_id")` that names neither the cause nor a cure.
///
/// Restore the unconditional `whole_number(row, "parent_id")` in `parents_before_children` and
/// this fails with exactly that error. The column is dropped from the source rather than a
/// pre-0010 database being built by hand, so the rest of the schema stays whatever the
/// migrations actually produce.
#[tokio::test]
async fn a_source_without_the_parent_id_column_still_copies() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    // Two objects and an activity, so the copy has both an ordering decision to make in
    // `objects` and a foreign key into it to satisfy afterwards.
    let golf = app.create_object(&app.client, "Golf", Some("km")).await;
    app.create_object(&app.client, "Bike", None).await;
    app.create_activity(&golf["id"], "Ölwechsel").await;

    // The index has to go first: SQLite refuses to drop a column another object still indexes.
    sqlx::query("DROP INDEX idx_objects_parent").execute(&app.state.db).await.unwrap();
    sqlx::query("ALTER TABLE objects DROP COLUMN parent_id").execute(&app.state.db).await.unwrap();

    app.release_database().await;
    let dest = common::scratch_database().await;

    let report = logb::copy::run(&app.database_url(), &dest.url).await.unwrap();
    assert!(report.tables.iter().any(|(t, n)| t == "objects" && *n == 2), "{:?}", report.tables);

    // The destination is migrated, so it does have the column; every row arrives without one,
    // which is a table of roots -- exactly what a database with no hierarchy holds.
    let pool = logb::db::connect_existing(&dest.url).await.unwrap();
    let rows: Vec<(i64, String, Option<i64>)> =
        sqlx::query_as("SELECT id, name, parent_id FROM objects ORDER BY id")
            .fetch_all(&pool).await.unwrap();
    pool.close().await;
    let names: Vec<&str> = rows.iter().map(|(_, name, _)| name.as_str()).collect();
    assert_eq!(names, vec!["Golf", "Bike"], "both objects must have crossed: {rows:?}");
    assert!(rows.iter().all(|(_, _, parent)| parent.is_none()), "{rows:?}");
}
