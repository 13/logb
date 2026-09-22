use super::common;
use super::helpers::*;
use serde_json::json;

#[tokio::test]
async fn a_set_op_updates_the_row_and_is_logged() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-1", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Golf VII",
            "edited_at": after_now(2714460), "device_id": "phone"
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
    assert_eq!(body["results"][0]["outcome"], "accepted");
    assert!(body["server_time"].is_string());

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(name, "Golf VII");

    let logged: i64 =
        sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-1'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(logged, 1);
}

#[tokio::test]
async fn an_older_edit_is_superseded_but_still_recorded() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let newer = json!([{
        "client_op_id": "op-new", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Newer",
        "edited_at": after_now(2764860), "device_id": "phone"
    }]);
    let older = json!([{
        "client_op_id": "op-old", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Older",
        "edited_at": after_now(2678460), "device_id": "phone"
    }]);
    app.client
        .post(app.url("/sync/push"))
        .json(&push_body(newer))
        .send()
        .await
        .unwrap();
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(older))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();

    assert_eq!(body["results"][0]["outcome"], "superseded");
    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(name, "Newer", "the loser must not overwrite the winner");
    let logged: i64 =
        sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-old'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(logged, 1, "a superseded op is still part of the log");
}

#[tokio::test]
async fn a_replayed_push_is_idempotent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let batch = push_body(json!([{
        "client_op_id": "op-same", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Once",
        "edited_at": after_now(2678460), "device_id": "phone"
    }]));
    // Both attempts must answer `accepted`: the first because it genuinely applied, the
    // second because idempotency reports the earlier attempt's outcome, not a fresh
    // `superseded` from replaying against the clock its own first attempt just stamped.
    // Asserting only the log count below would pass just the same if the first attempt had
    // silently lost last-write-wins -- a `changes` row is written for `superseded` too (see
    // `an_older_edit_is_superseded_but_still_recorded`), so a log count of 1 alone does not
    // prove this op ever actually applied.
    for _ in 0..2 {
        let res = app
            .client
            .post(app.url("/sync/push"))
            .json(&batch)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    }
    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(name, "Once", "the op actually applied, not merely logged");

    let logged: i64 =
        sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-same'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(logged, 1, "the same op id lands exactly once");
}

#[tokio::test]
async fn timestamps_are_compared_chronologically_not_lexically() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    // An anchor safely in the future, floored to a whole second so "whole" and "frac" below
    // differ by exactly 500ms with nothing left to chance from `Utc::now()`'s own fraction.
    let anchor = chrono::DateTime::<chrono::Utc>::from_timestamp(
        (chrono::Utc::now() + chrono::Duration::seconds(600)).timestamp(),
        0,
    )
    .unwrap();
    // No fraction at all -- what a client that never bothered with sub-second precision sends.
    let whole = anchor.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    // Half a second LATER, written with a fraction. Compared as raw strings the fractional one
    // loses ('.' < 'Z'), so a lexical rule would keep "Early"; canonicalization is what stops
    // that.
    let frac = (anchor + chrono::Duration::milliseconds(500))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    for (id, value, at) in [
        ("op-whole", "Early", whole.as_str()),
        ("op-frac", "Later", frac.as_str()),
    ] {
        let res = app
            .client
            .post(app.url("/sync/push"))
            .json(&push_body(json!([{
                "client_op_id": id, "entity": "object", "entity_uuid": uuid,
                "op": "set", "field": "name", "value": value,
                "edited_at": at, "device_id": "phone"
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
    }

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(name, "Later", "the chronologically later edit must win");

    // The same instant as `whole`, in offset-notation form (`+00:00` rather than `Z`); must
    // not re-win over the "Later" (`frac`) value it is chronologically earlier than.
    let offset_form = anchor.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-offset", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Earlier still",
            "edited_at": offset_form, "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"],
        "superseded"
    );

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-junk", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Nonsense",
            "edited_at": "last thursday", "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"],
        "rejected"
    );

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(name, "Later");
}

#[tokio::test]
async fn a_field_outside_the_whitelist_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-evil", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "user_id", "value": 2,
            "edited_at": after_now(2678460), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");
}

/// A device that was offline across the upgrade arrives with an operation naming a column that
/// no longer exists. It must be rejected on its own -- a batch that 500s is retried identically
/// forever, which is how one bad operation once wedged a client permanently.
#[tokio::test]
async fn an_operation_naming_the_removed_field_is_rejected_not_fatal() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "op-1", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "category", "value": "auto",
              "edited_at": after_now(60), "device_id": "phone" },
            { "client_op_id": "op-2", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "name", "value": "Golf VII",
              "edited_at": after_now(60), "device_id": "phone" }
        ])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "one bad op must not fail the batch: {}",
        res.text().await.unwrap()
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");
    assert_eq!(
        body["results"][1]["outcome"], "accepted",
        "the good op in the same batch applies"
    );
}

#[tokio::test]
async fn a_foreign_key_field_cannot_point_at_another_users_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let victim_object = car["id"].as_i64().unwrap();
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"%PDF-1.4 fake".to_vec())
            .file_name("invoice.pdf")
            .mime_str("application/pdf")
            .unwrap(),
    );
    let res = app
        .client
        .post(app.url(&format!("/objects/{victim_object}/attachments")))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        201,
        "upload failed: {}",
        res.text().await.unwrap()
    );
    let victim_attachment = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    // A second account, with an object of its own, tries to adopt the first account's
    // attachment as its cover image.
    let mallory = app.create_user_client("mallory", "another password").await;
    let theirs = app.create_object(&mallory, "Bike", None).await;
    let their_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(theirs["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let res = mallory
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-steal", "entity": "object", "entity_uuid": their_uuid,
            "op": "set", "field": "cover_attachment_id", "value": victim_attachment,
            "edited_at": after_now(15638460), "device_id": "mallory-phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");

    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE client_uuid = $1")
            .bind(&their_uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(
        cover.is_none(),
        "the cross-account reference must not have landed"
    );
}

#[tokio::test]
async fn one_user_cannot_push_at_another_users_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let other = app.create_user_client("mallory", "another password").await;
    let res = other
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-cross", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Stolen",
            "edited_at": "2030-01-01T00:00:00Z", "device_id": "mallory-phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
        .bind(&uuid)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(name, "Golf", "an unrelated user changed nothing");
}

#[tokio::test]
async fn results_stay_in_the_order_the_ops_were_sent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    // A batch that mixes ops rejected at different stages -- an unparseable timestamp, a field
    // off the whitelist -- with ones that land. `results[i]` must still describe `ops[i]`, so a
    // client can line the two lists up by position and not only by `client_op_id`.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([
            { "client_op_id": "ord-1", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "name", "value": "First",
              "edited_at": after_now(5097660), "device_id": "phone" },
            { "client_op_id": "ord-2", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "category", "value": "car",
              "edited_at": "not a timestamp", "device_id": "phone" },
            { "client_op_id": "ord-3", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "description", "value": "Mine",
              "edited_at": after_now(5097660), "device_id": "phone" },
            { "client_op_id": "ord-4", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "user_id", "value": 2,
              "edited_at": after_now(5097660), "device_id": "phone" },
            { "client_op_id": "ord-5", "entity": "object", "entity_uuid": uuid,
              "op": "set", "field": "name", "value": "Superseded by ord-1",
              "edited_at": after_now(60), "device_id": "phone" }
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
    let results = body["results"].as_array().unwrap();

    let seen: Vec<(&str, &str)> = results
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
            ("ord-1", "accepted"),
            ("ord-2", "rejected"),
            ("ord-3", "accepted"),
            ("ord-4", "rejected"),
            ("ord-5", "superseded"),
        ],
        "results[i] must describe ops[i]"
    );
}

#[tokio::test]
async fn two_users_can_use_the_same_client_op_id() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let mine = app.create_object(&app.client, "Golf", Some("km")).await;
    let my_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(mine["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let other = app.create_user_client("mallory", "another password").await;
    let theirs = app.create_object(&other, "Bike", None).await;
    let their_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(theirs["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    // Op ids are minted by clients, so nothing stops two accounts picking the same one. Each
    // push touches only its own object, so both writes must land: if idempotency were judged
    // on `client_op_id` alone, the second account would be told `accepted` and its write
    // silently dropped -- a lost write reported as success.
    for (client, uuid, name) in [
        (&app.client, &my_uuid, "Golf VII"),
        (&other, &their_uuid, "Brompton"),
    ] {
        let res = client
            .post(app.url("/sync/push"))
            .json(&push_body(json!([{
                "client_op_id": "op-shared", "entity": "object", "entity_uuid": uuid,
                "op": "set", "field": "name", "value": name,
                "edited_at": after_now(10368060), "device_id": "phone"
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
    }

    for (uuid, expected) in [(&my_uuid, "Golf VII"), (&their_uuid, "Brompton")] {
        let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = $1")
            .bind(uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        assert_eq!(&name, expected, "each account's own write must land");
    }

    // Both ops really are in the log, one row per account, not one row shared by the pair.
    let logged: i64 =
        sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-shared'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(logged, 2, "the log is keyed by (user_id, client_op_id)");
}

#[tokio::test]
async fn a_foreign_key_field_rejects_a_value_that_is_not_an_id() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(object_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(png())
            .file_name("cover.png")
            .mime_str("image/png")
            .unwrap(),
    );
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        201,
        "upload failed: {}",
        res.text().await.unwrap()
    );
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-cover", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "cover_attachment_id", "value": attachment_id,
            "edited_at": after_now(13046460), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"],
        "accepted"
    );

    // A foreign key holds an id or nothing. SQLite would happily store this string in the
    // integer column, so the ownership check -- which only looks at integers -- must not be
    // the only thing standing between a junk value and the write.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-junk-fk", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "cover_attachment_id", "value": "not-an-id",
            "edited_at": after_now(13132860), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");

    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        cover,
        Some(attachment_id),
        "the column must be untouched by the rejected op"
    );

    // Null is still how a client clears the reference, and must not be caught by the above.
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-clear-fk", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "cover_attachment_id", "value": null,
            "edited_at": after_now(13219260), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted", "{body}");
    let cover: Option<i64> =
        sqlx::query_scalar("SELECT cover_attachment_id FROM objects WHERE client_uuid = $1")
            .bind(&uuid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(cover.is_none(), "null clears the reference");
}
