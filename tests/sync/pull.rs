use super::common;
use super::helpers::*;
use serde_json::json;

#[tokio::test]
async fn pull_returns_ops_after_the_cursor_and_advances_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    for (n, name) in [("op-a", "First"), ("op-b", "Second")] {
        app.client
            .post(app.url("/sync/push"))
            .json(&push_body(json!([{
                "client_op_id": n, "entity": "object", "entity_uuid": uuid,
                "op": "set", "field": "name", "value": name,
                // op-a strictly earlier than op-b, one day apart -- the gap is what pull's
                // ordering is pinned against, not the absolute date.
                "edited_at": after_now(5097660 + if n == "op-a" { 0 } else { 86400 }),
                "device_id": "phone"
            }])))
            .send()
            .await
            .unwrap();
    }

    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // The object's own `create` is now logged too (task 9), ahead of the two `set` ops.
    assert_eq!(body["changes"].as_array().unwrap().len(), 3);
    assert_eq!(body["complete"], true);
    assert!(body["server_time"].is_string());
    let next = body["next_seq"].as_i64().unwrap();
    let epoch = body["epoch"].as_str().unwrap();

    let body: serde_json::Value = app
        .client
        .get(app.url(&format!("/sync/pull?since={next}&epoch={epoch}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        body["changes"].as_array().unwrap().len(),
        0,
        "the cursor is exhausted"
    );
}

#[tokio::test]
async fn pull_pages_and_reports_incompleteness() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    for i in 0..3 {
        app.client
            .post(app.url("/sync/push"))
            .json(&push_body(json!([{
                "client_op_id": format!("op-{i}"), "entity": "object", "entity_uuid": uuid,
                "op": "set", "field": "description", "value": format!("note {i}"),
                // Each op a day after the last -- the gaps are what paging is pinned against.
                "edited_at": after_now(7776060 + i as i64 * 86400), "device_id": "phone"
            }])))
            .send()
            .await
            .unwrap();
    }

    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/pull?since=0&limit=2"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 2);
    assert_eq!(body["complete"], false, "more remains behind the page");
}

/// Every other pull test checks counts; none pins a single field of a `ChangeRow`, so the
/// whole wire contract a phone client is about to be built against is unpinned. `value` is the
/// sharpest trap: `changes.value` stores `op.value.to_string()` (`api::sync::push`), the JSON
/// TEXT of the op's own value, so a plain string arrives double-encoded -- a JSON string
/// literal sitting inside the outer JSON string.
#[tokio::test]
async fn a_pulled_change_rows_fields_match_the_op_that_produced_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf VI", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let edited_at = after_now(60);
    let res = app
        .client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-wire", "entity": "object", "entity_uuid": &uuid,
            "op": "set", "field": "name", "value": "Golf VII",
            "edited_at": &edited_at, "device_id": "phone-42"
        }])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    assert_eq!(
        res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"],
        "accepted"
    );

    let body: serde_json::Value = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let row = body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "set" && c["field"] == "name")
        .unwrap_or_else(|| panic!("no set/name row in {body}"));

    assert_eq!(row["entity"], "object");
    assert_eq!(row["entity_uuid"], uuid);
    assert_eq!(row["op"], "set");
    assert_eq!(row["field"], "name");
    assert_eq!(
        row["value"], "\"Golf VII\"",
        "a string value arrives double-encoded"
    );
    // `edited_at` was already millisecond-precision (`after_now` uses the same
    // `SecondsFormat::Millis` canonical form), so this also pins canonicalization as
    // idempotent on an already-canonical value, not merely present.
    assert_eq!(row["edited_at"], edited_at);
    assert_eq!(row["device_id"], "phone-42");
    assert!(row["seq"].as_i64().unwrap() > 0);
}

/// Chains partial pages end to end and confirms every row surfaces exactly once, in `seq`
/// order, with `complete` true only on the last one -- the property a phone client actually
/// depends on to catch up without gaps or duplicates.
#[tokio::test]
async fn pull_pages_chain_to_deliver_every_row_exactly_once_in_seq_order() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    // 7 pushed sets, plus the object's own `create` already in the log: 8 rows, which does not
    // divide evenly by the page size below, so the last page is genuinely partial rather than
    // landing on the boundary by luck.
    for i in 0..7 {
        app.client
            .post(app.url("/sync/push"))
            .json(&push_body(json!([{
                "client_op_id": format!("op-{i}"), "entity": "object", "entity_uuid": uuid,
                "op": "set", "field": "description", "value": format!("note {i}"),
                "edited_at": after_now(60 + i as i64), "device_id": "phone"
            }])))
            .send()
            .await
            .unwrap();
    }

    // Fetched once via bootstrap, off to the side, so it does not add a row to `seen` the way
    // an extra pull would.
    let epoch: serde_json::Value = app
        .client
        .get(app.url("/sync/bootstrap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let epoch = epoch["epoch"].as_str().unwrap();

    let mut seen: Vec<i64> = Vec::new();
    let mut since = 0i64;
    loop {
        let body: serde_json::Value = app
            .client
            .get(app.url(&format!("/sync/pull?since={since}&limit=3&epoch={epoch}")))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let changes = body["changes"].as_array().unwrap();
        let complete = body["complete"].as_bool().unwrap();
        assert!(
            complete || changes.len() == 3,
            "an incomplete page must be full: {body}"
        );
        for c in changes {
            seen.push(c["seq"].as_i64().unwrap());
        }
        since = body["next_seq"].as_i64().unwrap();
        if complete {
            break;
        }
    }

    assert_eq!(
        seen.len(),
        8,
        "1 create + 7 sets, across however many pages it took"
    );
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, seen, "every row exactly once, already in seq order");
}

#[tokio::test]
async fn pull_never_leaks_another_users_changes() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    app.client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-ben", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Ben's",
            "edited_at": after_now(10368060), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();

    let other = app.create_user_client("mallory", "another password").await;
    let body: serde_json::Value = other
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn a_cursor_before_the_horizon_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    app.client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-kept", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Kept",
            "edited_at": after_now(13046460), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();

    // The epoch this database is currently on, fetched before the purge is simulated below --
    // that only rewrites `changes`, not `settings`, so the epoch is unaffected. Carrying it on
    // the request below is what pins this 410 on the horizon rule specifically: without it, an
    // un-epoched non-zero `since` is refused by the epoch check regardless of the horizon, and
    // this test would pass even with the horizon rule deleted entirely.
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

    // Simulate a purge having removed everything before this row -- including the object's
    // own `create`, which is in the log too now (task 9), or the horizon would still read as
    // the create row's untouched seq and this cursor would look current rather than stale.
    sqlx::query("DELETE FROM changes WHERE client_op_id != 'op-kept'")
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE changes SET seq = 500 WHERE client_op_id = 'op-kept'")
        .execute(&app.state.db)
        .await
        .unwrap();

    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since=1&epoch={epoch}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        410,
        "a stale cursor must be told to re-bootstrap"
    );
}

/// `api::sync::pull` rejects `since < horizon - 1`, not `since <= horizon` or any other
/// off-by-one -- `since == horizon - 1` means the client has already seen the row one below
/// the oldest surviving `seq`, so nothing was purged out from under it; `since == horizon - 2`
/// means it is missing exactly the row the horizon itself sits on. Mutation testing proved the
/// existing tests do not pin the exact boundary: changing `- 1` to `- 2` (the data-losing
/// direction -- a client landing exactly on the boundary would be told 200 and silently
/// resume past a purged row) left `cargo test --test sync` green.
#[tokio::test]
async fn a_cursor_one_below_the_horizon_is_accepted_and_two_below_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    app.client
        .post(app.url("/sync/push"))
        .json(&push_body(json!([{
            "client_op_id": "op-kept", "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": "Kept",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .send()
        .await
        .unwrap();

    // The epoch this database is currently on, fetched before the purge is simulated below --
    // that only rewrites `changes`, not `settings`, so the epoch is unaffected.
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

    // As in the sibling test above: simulate a purge leaving exactly one row, at seq 500, so
    // the horizon (the oldest surviving seq) is 500.
    sqlx::query("DELETE FROM changes WHERE client_op_id != 'op-kept'")
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE changes SET seq = 500 WHERE client_op_id = 'op-kept'")
        .execute(&app.state.db)
        .await
        .unwrap();

    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since=499&epoch={epoch}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "since == horizon - 1 (499) has missed nothing and must be accepted"
    );

    // Carrying the epoch here too: without it, this 410 comes from the epoch check (no epoch
    // on a non-zero `since`), not the horizon rule this test exists to pin.
    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since=498&epoch={epoch}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        410,
        "since == horizon - 2 (498) has missed seq 499 and must be refused"
    );
}

/// `seq` is shared by every account, so a gap wider than one below the horizon does not mean
/// this user lost more than one row -- it can just as easily be a rejected push's burned
/// `client_op_id` claim, or another account's own op, neither of which was ever this user's
/// data to lose. `feed::horizon` cannot tell those apart from a genuinely purged row of this
/// user's own; comparing against `feed::retention_floor` instead does not need to, because
/// nothing above that floor has been purged for anyone.
///
/// Mallory's object anchors the floor low and is never touched. Ben's own log is then trimmed
/// down to a single surviving row far above his cursor, with three rejected ops' burned claims
/// sitting in the gap between them -- exactly the shape the old `since < horizon - 1` rule
/// mistook for missing data, because it judged staleness against Ben's own horizon rather than
/// what the server still retains for anyone. Before the fix this answers 410; the fix must
/// answer 200 and actually resume from where Ben left off.
#[tokio::test]
async fn a_gap_from_a_rejected_push_below_the_boundary_does_not_force_a_rebootstrap() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // Anchors the retention floor at a low `seq` that is never removed, so it stays well below
    // anything Ben's own cursor could be judged against once his own earlier rows are gone.
    let mallory = app.create_user_client("mallory", "another password").await;
    app.create_object(&mallory, "Mallory's ride", None).await;

    let car = app.create_object(&app.client, "Golf", Some("km")).await;

    // The epoch, fetched before any of the manipulation below -- it only touches `changes`, not
    // `settings`, so it stays constant, but a non-zero `since` on a real pull needs one either
    // way.
    let epoch: String = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()["epoch"]
        .as_str()
        .unwrap()
        .to_string();

    // The row Ben's client already has -- this becomes its cursor.
    let kept = app.one_set_op(&car, "Kept", "op-kept").await;
    assert_eq!(app.push_raw(&kept).await.status(), 200);
    let since: i64 = sqlx::query_scalar("SELECT seq FROM changes WHERE client_op_id = 'op-kept'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    // Three ops that never become rows: an unknown `entity_uuid` is rejected by
    // `apply::apply_op` before it touches any table, so each burns exactly the `changes` claim
    // its own `client_op_id` insert made and nothing else.
    for i in 0..3 {
        let body = push_body(json!([{
            "client_op_id": format!("op-burn-{i}"), "entity": "object",
            "entity_uuid": "does-not-exist", "op": "set", "field": "name", "value": "x",
            "edited_at": after_now(60), "device_id": "phone"
        }]));
        let res: serde_json::Value = app.push_raw(&body).await.json().await.unwrap();
        assert_eq!(
            res["results"][0]["outcome"], "rejected",
            "the burn setup itself must reject, or nothing here pins a real gap"
        );
    }

    // One more real row, which will end up as Ben's own new horizon.
    let new = app.one_set_op(&car, "New", "op-new").await;
    assert_eq!(app.push_raw(&new).await.status(), 200);

    // Simulate the purge having reclaimed everything of Ben's own older than `op-new` -- his
    // object's own `create` row and `op-kept` included -- while leaving Mallory's row (and
    // everything else) alone.
    let ben_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'ben'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM changes WHERE user_id = $1 AND client_op_id != 'op-new'")
        .bind(ben_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    let new_horizon: i64 = sqlx::query_scalar("SELECT min(seq) FROM changes WHERE user_id = $1")
        .bind(ben_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(
        since < new_horizon - 1,
        "the setup must actually widen the gap past what the old rule tolerated, or this test \
         proves nothing: since={since}, horizon={new_horizon}"
    );

    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since={since}&epoch={epoch}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "a burned claim and another account's own row never held data of Ben's to lose -- \
         resuming from his own last-seen row must not force a re-bootstrap"
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        body["changes"].as_array().unwrap().len(),
        1,
        "resuming must actually return the one row Ben has not seen yet"
    );
    assert_eq!(
        body["next_seq"], new_horizon,
        "the one row returned must be his new horizon"
    );
}

#[tokio::test]
async fn a_cursor_against_an_emptied_log_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // The epoch, so the assertion below is pinned on the horizon rule (empty log, so horizon
    // is 0) rather than on the epoch check, which a bare non-zero `since` would also trip.
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

    // A non-zero cursor can only have come from ops that existed, so an empty log means they
    // were purged. Answering 200 here would let the client believe it is current forever.
    let res = app
        .client
        .get(app.url(&format!("/sync/pull?since=7&epoch={epoch}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 410);

    // A first pull is still legal against the same empty log.
    let res = app
        .client
        .get(app.url("/sync/pull?since=0"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}

