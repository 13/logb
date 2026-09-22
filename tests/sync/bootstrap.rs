use super::common;
use super::helpers::*;

#[tokio::test]
async fn bootstrap_returns_live_rows_and_a_resumable_cursor() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let keep = app.create_object(&app.client, "Golf", Some("km")).await;
    let drop = app.create_object(&app.client, "Old Bike", None).await;
    let drop_id = drop["id"].as_i64().unwrap();
    assert_eq!(
        app.client
            .delete(app.url(&format!("/objects/{drop_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/bootstrap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let objects = body["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 1, "the tombstoned object is absent");
    assert_eq!(objects[0]["name"], keep["name"]);
    assert!(
        objects[0]["client_uuid"].is_string(),
        "rows are addressable by uuid"
    );
    assert!(body["seq"].is_i64());
    assert!(body["server_time"].is_string());

    // The cursor is immediately usable, together with the epoch it was issued alongside.
    let seq = body["seq"].as_i64().unwrap();
    let epoch = body["epoch"].as_str().unwrap();
    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since={seq}&epoch={epoch}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn bootstrap_is_scoped_to_the_caller() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let other = app.create_user_client("mallory", "another password").await;
    let body: serde_json::Value = other
        .get(app.url("/sync/bootstrap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["objects"].as_array().unwrap().len(), 0);
}

/// The sibling above only pins `objects` -- `feed::snapshot`'s activities/reminders/
/// attachments/files queries are four more statements, each with its own `WHERE`, and nothing
/// about the objects check proves any of them are scoped. Mutation testing confirmed it:
/// stripping `o.user_id = ?` and `o.deleted_at IS NULL` from all four left the whole suite
/// green. This pins two failure modes those clauses guard against: another account's rows
/// leaking in (the `user_id`/join half), and a LIVE child surviving in a snapshot because its
/// parent object was tombstoned without it (the `o.deleted_at IS NULL` half) -- which the
/// child's own `deleted_at IS NULL` filter cannot catch on its own, since the child really is
/// live.
#[tokio::test]
async fn bootstrap_scopes_every_child_table_and_hides_a_live_child_of_a_tombstoned_object() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    object_with_children(&app, &app.client, "Golf").await;

    // -- Cross-account: a second account with an object of its own, and none of ben's rows,
    // must see none of ben's activities, reminders, attachments or files.
    let mallory = app.create_user_client("mallory", "another password").await;
    app.create_object(&mallory, "Bike", None).await;

    let body: serde_json::Value = mallory
        .get(app.url("/sync/bootstrap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for table in ["activities", "reminders", "attachments", "files"] {
        assert_eq!(
            body[table].as_array().unwrap().len(),
            0,
            "mallory's bootstrap must not carry ben's {table}: {body}"
        );
    }

    // -- A live child of a tombstoned object. `api::objects::delete` always cascades the
    // tombstone to every child, so reaching "object gone, child still live" needs a raw update
    // that bypasses the cascade -- exactly the shape a bug in that cascade (or a hand-run
    // migration) would leave behind, and precisely what the join's `o.deleted_at IS NULL` has
    // to catch since the child rows here are otherwise ordinary and live.
    let (orphan_object, orphan_activity, orphan_reminder, orphan_attachment) =
        object_with_children(&app, &app.client, "Orphaned").await;
    sqlx::query("UPDATE objects SET deleted_at = $1 WHERE id = $2")
        .bind(logb::db::now())
        .bind(orphan_object)
        .execute(&app.state.db)
        .await
        .unwrap();
    let object_deleted: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM objects WHERE id = $1")
            .bind(orphan_object)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(
        object_deleted.is_some(),
        "fixture setup: the object must be tombstoned"
    );
    for (table, id) in [
        ("activities", orphan_activity),
        ("reminders", orphan_reminder),
        ("attachments", orphan_attachment),
    ] {
        let deleted: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT deleted_at FROM {table} WHERE id = $1"
        )))
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        assert!(
            deleted.is_none(),
            "fixture setup: the {table} row must still be live"
        );
    }

    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/bootstrap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for table in ["activities", "reminders", "attachments"] {
        assert!(
            body[table]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["object_id"] != orphan_object),
            "a live child of a tombstoned object must not appear in {table}: {body}"
        );
    }
}


#[tokio::test]
async fn a_pull_carrying_a_stale_epoch_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let _ = car;

    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let epoch = body["epoch"]
        .as_str()
        .expect("pull states the epoch")
        .to_string();
    let next = body["next_seq"].as_i64().unwrap();
    assert!(next > 0, "the REST create is already in the feed");

    // The cursor is current and the epoch matches: ordinary catch-up.
    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since={next}&epoch={epoch}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // Same cursor, an epoch from a different database: the numbers no longer mean what the
    // device thinks they mean.
    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since={next}&epoch=not-this-database")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 410);

    // A non-zero cursor with no epoch at all is the same failure: a client that cannot say
    // which database it is resuming against cannot safely resume.
    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since={next}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 410);

    // A first pull carries no cursor, so it needs no epoch.
    assert_eq!(
        app.client
            .get(app.url("/sync/pull?since=0"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    // The rule is `since > 0 && epoch mismatch`, on a single `&&` -- pin that a garbage epoch
    // alongside `since=0` still passes, so a future edit cannot accidentally start checking the
    // epoch on a first pull too.
    let res = app
        .client
        .get(app.url("/sync/pull?since=0&epoch=garbage"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "since=0 needs no epoch, garbage or otherwise"
    );
}

#[tokio::test]
async fn bootstrap_states_the_epoch_it_belongs_to() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/bootstrap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let epoch = body["epoch"].as_str().expect("bootstrap states the epoch");

    // The pair is usable together: the seq and epoch a bootstrap hands out are accepted by pull.
    let seq = body["seq"].as_i64().unwrap();
    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since={seq}&epoch={epoch}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn rotating_the_epoch_forces_every_device_to_re_bootstrap() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let epoch = body["epoch"].as_str().unwrap().to_string();
    let next = body["next_seq"].as_i64().unwrap();

    let fresh = logb::sync::epoch::rotate(&app.state.db).await.unwrap();
    assert_ne!(fresh, epoch, "rotation produces a different epoch");

    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since={next}&epoch={epoch}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        410,
        "the device's epoch is now the old database's"
    );
}

/// `rotate` must not report success when nothing actually changed. A plain `UPDATE ... WHERE
/// key = 'sync_epoch'` matches zero rows if that row is ever absent, and would still return a
/// freshly minted uuid to the caller -- `--restore` would print "sync epoch is now ..." and
/// exit 0 while the database goes on advertising its old identity (or, here, none at all, which
/// then makes every pull 500 through `current`'s `fetch_one`).
#[tokio::test]
async fn rotate_heals_a_missing_row_instead_of_silently_reporting_a_fake_success() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    sqlx::query("DELETE FROM settings WHERE key = 'sync_epoch'")
        .execute(&app.state.db)
        .await
        .unwrap();

    let fresh = logb::sync::epoch::rotate(&app.state.db)
        .await
        .expect("rotate must not silently no-op when the row is missing");

    let value: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'sync_epoch'")
        .fetch_one(&app.state.db)
        .await
        .expect("rotate reported success, but the row it claims to have set is not there");
    assert_eq!(
        value, fresh,
        "the row actually on disk must match what rotate reported"
    );
}
