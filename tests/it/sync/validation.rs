use super::common;
use super::helpers::*;
use serde_json::json;

/// Every other push test in this file sets a field on an `object`, whose ownership is a column
/// read. The three child entities each reach `objects.user_id` through a join of their own, and
/// those joins are the whole of the authorization check for them.
///
/// Asserting only that a cross-account push comes back `rejected` would pin almost none of
/// that. A join that matched NOTHING would satisfy it while breaking every legitimate push at
/// a child row, so this test has a positive half as well: the owning account's pushes at its
/// own reminder and attachment must be `accepted` and must actually land. And the rejection
/// REASON is asserted, not just the outcome, because "unknown entity_uuid" from an ownership
/// check and the same words from a lookup that can never find anything are the two answers
/// this test exists to tell apart.
///
/// The fixture exists for the third failure mode: a join on the wrong COLUMN. If two accounts
/// each create one object and one child of each kind, every child row's id equals its own
/// `object_id`, so `o.id = r.id` reads exactly like `o.id = r.object_id` and the typo is
/// invisible. Giving the second account rows of its own AND a spare object offsets the two id
/// sequences: each of the victim's child rows then carries an id that names the OTHER account's
/// object, so a wrong-column join both accepts a push it must reject and rejects one it must
/// accept.
#[tokio::test]
async fn one_user_cannot_push_at_another_users_activity_reminder_or_attachment() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // Mallory first, and with one more object than child sets, so no id lines up with itself.
    let mallory = app.create_user_client("mallory", "another password").await;
    app.create_object(&mallory, "Spare", None).await;
    object_with_children(&app, &mallory, "Brompton").await;

    let (object_id, activity_id, reminder_id, attachment_id) =
        object_with_children(&app, &app.client, "Golf").await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;
    let reminder_uuid = client_uuid(&app.state.db, "reminders", reminder_id).await;
    let attachment_uuid = client_uuid(&app.state.db, "attachments", attachment_id).await;

    // The premise the fixture buys, stated so it fails loudly rather than quietly rotting if
    // `object_with_children` ever changes what it creates.
    let mallory_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'mallory'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    for (label, child_id) in [
        ("activity", activity_id),
        ("reminder", reminder_id),
        ("attachment", attachment_id),
    ] {
        assert_ne!(
            child_id, object_id,
            "the {label} id must not equal its own object_id"
        );
        let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM objects WHERE id = $1")
            .bind(child_id)
            .fetch_optional(&app.state.db)
            .await
            .unwrap();
        assert_eq!(
            owner,
            Some(mallory_id),
            "the {label} id must name the other account's object, or a join on the wrong \
             column would give the same answer as the right one"
        );
    }

    let res = mallory
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "op-act", "entity": "activity", "entity_uuid": activity_uuid,
              "op": "set", "field": "title", "value": "Stolen activity",
              "edited_at": "2030-01-01T00:00:00Z", "device_id": "mallory-phone" },
            { "client_op_id": "op-rem", "entity": "reminder", "entity_uuid": reminder_uuid,
              "op": "set", "field": "title", "value": "Stolen reminder",
              "edited_at": "2030-01-01T00:00:00Z", "device_id": "mallory-phone" },
            { "client_op_id": "op-att", "entity": "attachment", "entity_uuid": attachment_uuid,
              "op": "set", "field": "caption", "value": "Stolen caption",
              "edited_at": "2030-01-01T00:00:00Z", "device_id": "mallory-phone" }
        ])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    for i in 0..3 {
        assert_eq!(body["results"][i]["outcome"], "rejected", "{body}");
        // The reason an ownership check gives. A lookup that found nothing at all says the
        // same thing, which is exactly why the positive half below has to exist too.
        assert_eq!(
            body["results"][i]["reason"], "unknown entity_uuid",
            "{body}"
        );
    }

    let title: String = sqlx::query_scalar("SELECT title FROM activities WHERE id = $1")
        .bind(activity_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        title, "Timing belt",
        "another account's activity is untouched"
    );
    let title: String = sqlx::query_scalar("SELECT title FROM reminders WHERE id = $1")
        .bind(reminder_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(title, "Service", "another account's reminder is untouched");
    let caption: String = sqlx::query_scalar("SELECT caption FROM attachments WHERE id = $1")
        .bind(attachment_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(caption, "", "another account's attachment is untouched");

    // A rejected op is not part of the log, so it must not reach a pull feed either.
    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE client_op_id IN ('op-act', 'op-rem', 'op-att')",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(logged, 0, "a rejected op is never logged");

    // The positive half. Each of the same three joins now has to FIND the row: an ownership
    // check that rejects everything is not an ownership check, and the rejections above cannot
    // tell the difference on their own.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "mine-act", "entity": "activity", "entity_uuid": activity_uuid,
              "op": "set", "field": "title", "value": "Timing belt done",
              "edited_at": after_now(2678460), "device_id": "ben-phone" },
            { "client_op_id": "mine-rem", "entity": "reminder", "entity_uuid": reminder_uuid,
              "op": "set", "field": "title", "value": "Service booked",
              "edited_at": after_now(2678460), "device_id": "ben-phone" },
            { "client_op_id": "mine-att", "entity": "attachment", "entity_uuid": attachment_uuid,
              "op": "set", "field": "caption", "value": "The old belt",
              "edited_at": after_now(2678460), "device_id": "ben-phone" }
        ])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    for i in 0..3 {
        assert_eq!(body["results"][i]["outcome"], "accepted", "{body}");
    }

    let title: String = sqlx::query_scalar("SELECT title FROM activities WHERE id = $1")
        .bind(activity_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(title, "Timing belt done", "the owner's own write must land");
    let title: String = sqlx::query_scalar("SELECT title FROM reminders WHERE id = $1")
        .bind(reminder_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(title, "Service booked", "the owner's own write must land");
    let caption: String = sqlx::query_scalar("SELECT caption FROM attachments WHERE id = $1")
        .bind(attachment_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(caption, "The old belt", "the owner's own write must land");
}

#[tokio::test]
async fn a_delete_op_tombstones_the_row_and_the_api_stops_serving_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_, activity_id, _, _) = object_with_children(&app, &app.client, "Golf").await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;

    assert_eq!(
        app.client
            .get(app.url(&format!("/activities/{activity_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        200,
        "the row is readable before the delete"
    );

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-del", "entity": "activity", "entity_uuid": activity_uuid,
            "op": "delete", "edited_at": after_now(7776060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM activities WHERE id = $1")
        .bind(activity_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(rows, 1, "the row survives; only deleted_at is set");
    let deleted: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM activities WHERE id = $1")
            .bind(activity_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(deleted.is_some(), "the delete op tombstones the row");

    assert_eq!(
        app.client
            .get(app.url(&format!("/activities/{activity_id}")))
            .send()
            .await
            .unwrap()
            .status(),
        404,
        "a tombstoned activity reads as absent"
    );

    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE client_op_id = 'op-del' AND op = 'delete'",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(
        logged, 1,
        "the delete is in the log for other devices to pull"
    );
}

/// A constraint violation used to escape as `sqlx::Error`, which the error mapper turns into a
/// 500. That rolled the whole transaction back, so ops already accepted in the same batch were
/// discarded, and the client's identical retry hit the same op and the same 500 forever: sync
/// stalled permanently on one op the client had no way to identify.
#[tokio::test]
async fn a_constraint_violating_op_is_rejected_without_poisoning_the_batch() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid = client_uuid(&app.state.db, "objects", car["id"].as_i64().unwrap()).await;

    // Ops 2 and 3 violate a NOT NULL and a CHECK constraint respectively; 1 and 4 are ordinary
    // writes that must survive them.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "ok-before", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "description", "value": "Mine",
              "edited_at": after_now(7776060), "device_id": "phone" },
            { "client_op_id": "bad-null", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "name", "value": null,
              "edited_at": after_now(7776060), "device_id": "phone" },
            { "client_op_id": "bad-check", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "counter_unit", "value": "furlongs",
              "edited_at": after_now(7776060), "device_id": "phone" },
            { "client_op_id": "ok-after", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "type", "value": "motorcycle",
              "edited_at": after_now(7776060), "device_id": "phone" }
        ])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "the batch must not 500: {}",
        res.text().await.unwrap()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    let seen: Vec<(&str, &str)> = body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["client_op_id"].as_str().unwrap(),
                r["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            ("ok-before", "accepted"),
            ("bad-null", "rejected"),
            ("bad-check", "rejected"),
            ("ok-after", "accepted"),
        ],
        "one malformed op must not change any other op's outcome: {body}"
    );
    assert!(
        body["results"][1]["reason"]
            .as_str()
            .unwrap()
            .contains("name"),
        "the reason must name the field so the client can drop that op: {body}"
    );
    assert!(
        body["results"][2]["reason"]
            .as_str()
            .unwrap()
            .contains("counter_unit"),
        "the reason must name the field so the client can drop that op: {body}"
    );

    // The transaction stayed usable: the ops either side of the failures really committed, and
    // the failing statements changed nothing.
    let row: (String, String, Option<String>, String) = sqlx::query_as(
        "SELECT name, type, counter_unit, description FROM objects WHERE client_uuid = $1",
    )
    .bind(&uuid)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            "Golf".into(),
            "motorcycle".into(),
            Some("km".into()),
            "Mine".into()
        ),
        "accepted ops committed; rejected ops wrote nothing"
    );

    // `op = 'set'` excludes the object's own `create` row (task 9 logs REST creates too, under
    // a freshly minted client_op_id of its own) -- this assertion is about the pushed batch.
    let logged: Vec<String> =
        sqlx::query_scalar("SELECT client_op_id FROM changes WHERE op = 'set' ORDER BY seq")
            .fetch_all(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        logged,
        vec!["ok-before", "ok-after"],
        "only the accepted ops are logged"
    );
}

/// `as_i64()` returns `None` for a float or an out-of-range magnitude, and the old binding fed
/// that `None` straight to SQLite as NULL: the op reported `accepted` and advanced `field_clock`,
/// so the client's correction -- carrying the value's original, earlier `edited_at` -- then lost
/// the last-write-wins comparison and could never repair the row.
#[tokio::test]
async fn a_number_that_is_not_an_integer_is_rejected_and_leaves_the_clock_alone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, activity_id, _, _) = object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "num-float", "entity": "object", "entity_uuid": object_uuid,
              "op": "set", "field": "purchase_price_cents", "value": 1250.5,
              "edited_at": after_now(10454460), "device_id": "phone" },
            { "client_op_id": "num-huge", "entity": "activity", "entity_uuid": activity_uuid,
              "op": "set", "field": "counter_value", "value": 100000000000000000000000_i128 as f64,
              "edited_at": after_now(10454460), "device_id": "phone" }
        ])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    for (i, field) in [(0, "purchase_price_cents"), (1, "counter_value")] {
        assert_eq!(body["results"][i]["outcome"], "rejected", "{body}");
        assert!(
            body["results"][i]["reason"]
                .as_str()
                .unwrap()
                .contains(field),
            "the reason must name the field: {body}"
        );
    }

    // Neither column was written -- the old code stored NULL and called it success.
    let price: Option<i64> =
        sqlx::query_scalar("SELECT purchase_price_cents FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(
        price.is_none(),
        "the row still holds what the REST create put there"
    );
    let counter: Option<i64> =
        sqlx::query_scalar("SELECT counter_value FROM activities WHERE client_uuid = $1")
            .bind(&activity_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        counter,
        Some(1000),
        "the good value the REST create wrote must survive"
    );

    // The repair: the client resends the value it always had, carrying its ORIGINAL edited_at,
    // which is EARLIER than the rejected op's. That only wins if the rejected op left no
    // `field_clock` row behind.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "num-repair", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "purchase_price_cents", "value": 1250,
            "edited_at": after_now(10368060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        body["results"][0]["outcome"], "accepted",
        "the client can still repair: {body}"
    );
    let price: Option<i64> =
        sqlx::query_scalar("SELECT purchase_price_cents FROM objects WHERE client_uuid = $1")
            .bind(&object_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(price, Some(1250));

    // An integer is still an ordinary accepted value.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "num-int", "entity": "activity", "entity_uuid": activity_uuid,
            "op": "set", "field": "cost_cents", "value": 9900,
            "edited_at": after_now(10540860), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    let cost: Option<i64> =
        sqlx::query_scalar("SELECT cost_cents FROM activities WHERE client_uuid = $1")
            .bind(&activity_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(cost, Some(9900));
}

/// A JSON string bound into an INTEGER column used to be `accepted`. SQLite's INTEGER affinity
/// cannot convert `"abc"`, so it stored it verbatim as TEXT, and every read decodes that column
/// as `Option<i64>`: the object's activity list and the activity itself answered 500 from then
/// on. From then on, because the write advanced `field_clock` too, so the client's correction --
/// carrying the value's original, EARLIER `edited_at` -- came back `superseded`. One malformed
/// op from any authenticated client, and the row could never be read or repaired again.
#[tokio::test]
async fn a_value_of_the_wrong_type_for_its_column_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (object_id, activity_id, _, _) = object_with_children(&app, &app.client, "Golf").await;
    let object_uuid = client_uuid(&app.state.db, "objects", object_id).await;
    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        // A string into an INTEGER column: the unrepairable 500 above.
        { "client_op_id": "type-str-into-int", "entity": "activity", "entity_uuid": activity_uuid,
          "op": "set", "field": "counter_value", "value": "abc",
          "edited_at": after_now(10454460), "device_id": "phone" },
        // And the milder direction, which corrupts silently: SQLite stores `true` in a TEXT
        // column as '1', so the object's name would have become the string "1".
        { "client_op_id": "type-bool-into-text", "entity": "object", "entity_uuid": object_uuid,
          "op": "set", "field": "name", "value": true,
          "edited_at": after_now(10454460), "device_id": "phone" }
    ]))).send().await.unwrap();
    assert_eq!(
        res.status(),
        200,
        "push failed: {}",
        res.text().await.unwrap()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    for (i, field) in [(0, "counter_value"), (1, "name")] {
        assert_eq!(body["results"][i]["outcome"], "rejected", "{body}");
        assert!(
            body["results"][i]["reason"]
                .as_str()
                .unwrap()
                .contains(field),
            "the reason must name the field: {body}"
        );
    }

    // The columns still hold what they held, in the storage class they are declared with.
    //
    // `typeof` is SQLite's function, and so is the question behind it: only SQLite would have
    // stored the string `"abc"` in an INTEGER column in the first place, so only there is
    // "is this column still holding an integer?" something a test can ask. PostgreSQL cannot
    // put anything but a bigint in a bigint column -- a wrongly typed write is an error, not a
    // silently different storage class -- so the value is the whole of what is left to check.
    let counter: Option<i64> = match app.state.backend {
        logb::dialect::Backend::Sqlite => {
            let stored: (String, Option<i64>) = sqlx::query_as(
                "SELECT typeof(counter_value), counter_value FROM activities WHERE id = $1",
            )
            .bind(activity_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
            assert_eq!(
                stored.0, "integer",
                "the integer column is still an integer"
            );
            stored.1
        }
        logb::dialect::Backend::Postgres => {
            sqlx::query_scalar("SELECT counter_value FROM activities WHERE id = $1")
                .bind(activity_id)
                .fetch_one(&app.state.db)
                .await
                .unwrap()
        }
    };
    assert_eq!(
        counter,
        Some(1000),
        "the integer column still holds the value it held"
    );
    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE id = $1")
        .bind(object_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(name, "Golf", "the text column is untouched");

    // The reads that used to 500 on a corrupted row.
    for path in [
        format!("/objects/{object_id}/activities"),
        format!("/activities/{activity_id}"),
    ] {
        assert_eq!(
            app.client
                .get(app.url(&path))
                .send()
                .await
                .unwrap()
                .status(),
            200,
            "{path} must still decode"
        );
    }

    // And the clock did not move, so an edit carrying an EARLIER timestamp -- which is all a
    // correcting client has -- still wins.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "type-repair", "entity": "activity", "entity_uuid": activity_uuid,
            "op": "set", "field": "counter_value", "value": 2000,
            "edited_at": after_now(10368060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        body["results"][0]["outcome"], "accepted",
        "the rejected op left no clock: {body}"
    );
    let counter: Option<i64> =
        sqlx::query_scalar("SELECT counter_value FROM activities WHERE id = $1")
            .bind(activity_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(counter, Some(2000));
}

