mod common;
use serde_json::json;

fn act(date: &str, category: &str, counter: Option<i64>, cost: Option<i64>) -> serde_json::Value {
    json!({ "date": date, "category": category, "title": format!("{category} on {date}"),
            "notes": "", "counter_value": counter, "cost_cents": cost })
}

#[tokio::test]
async fn timeline_totals_and_counter() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/activities"));

    for a in [
        act("2024-01-10", "maintenance", Some(100_000), Some(25_000)),
        act("2024-06-01", "repair", Some(104_500), Some(80_000)),
        act("2024-03-15", "fuel", Some(102_000), None),
    ] {
        let res = app.client.post(&base).json(&a).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let list: Vec<serde_json::Value> = app
        .client
        .get(&base)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let dates: Vec<&str> = list.iter().map(|a| a["date"].as_str().unwrap()).collect();
    assert_eq!(
        dates,
        ["2024-06-01", "2024-03-15", "2024-01-10"],
        "newest first"
    );

    let obj: serde_json::Value = app
        .client
        .get(app.url(&format!("/objects/{id}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(obj["stats"]["total_cost_cents"], 105_000);
    assert_eq!(obj["stats"]["activity_count"], 3);
    assert_eq!(obj["stats"]["current_counter"], 104_500);

    let fuel: Vec<serde_json::Value> = app
        .client
        .get(format!("{base}?category=fuel"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fuel.len(), 1);
    let h1: Vec<serde_json::Value> = app
        .client
        .get(format!("{base}?from=2024-01-01&to=2024-03-31"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h1.len(), 2);

    let aid = list[0]["id"].as_i64().unwrap();
    let res = app
        .client
        .patch(app.url(&format!("/activities/{aid}")))
        .json(&act("2024-06-02", "repair", Some(104_600), Some(90_000)))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let one: serde_json::Value = app
        .client
        .get(app.url(&format!("/activities/{aid}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(one["cost_cents"], 90_000);

    assert_eq!(
        app.client
            .delete(app.url(&format!("/activities/{aid}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    let obj: serde_json::Value = app
        .client
        .get(app.url(&format!("/objects/{id}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(obj["stats"]["total_cost_cents"], 25_000);
    assert_eq!(obj["stats"]["current_counter"], 102_000);
}

#[tokio::test]
async fn validation() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let home = app.create_object(&app.client, "Home", None).await;
    let car_base = app.url(&format!("/objects/{}/activities", car["id"]));
    let home_base = app.url(&format!("/objects/{}/activities", home["id"]));
    for (base, body) in [
        (&car_base, act("2024-13-01", "repair", None, None)),
        (&car_base, act("2024-01-01", "party", None, None)),
        (
            &car_base,
            json!({ "date": "2024-01-01", "category": "repair", "title": "  " }),
        ),
        (&car_base, act("2024-01-01", "repair", Some(-5), None)),
        (&car_base, act("2024-01-01", "repair", None, Some(-1))),
        (&home_base, act("2024-01-01", "repair", Some(10), None)),
    ] {
        let res = app.client.post(base).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 400, "{body}");
    }
}

#[tokio::test]
async fn isolation() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let base = app.url(&format!("/objects/{}/activities", car["id"]));
    let a: serde_json::Value = app
        .client
        .post(&base)
        .json(&act("2024-01-01", "repair", None, Some(1)))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(anna.get(&base).send().await.unwrap().status(), 404);
    assert_eq!(
        anna.post(&base)
            .json(&act("2024-01-01", "repair", None, None))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        anna.get(app.url(&format!("/activities/{}", a["id"])))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        anna.patch(app.url(&format!("/activities/{}", a["id"])))
            .json(&act("2024-01-01", "repair", None, None))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        anna.delete(app.url(&format!("/activities/{}", a["id"])))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

/// A long timeline comes back a page at a time, with the unpaged total in a header so the
/// client knows whether to offer "show older".
#[tokio::test]
async fn the_activity_list_is_paged() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    for i in 0..7 {
        app.client.post(app.url(&format!("/objects/{id}/activities")))
            .json(&json!({ "date": format!("2024-01-{:02}", i + 1), "category": "fuel", "title": format!("Fill {i}") }))
            .send().await.unwrap();
    }

    let res = app
        .client
        .get(app.url(&format!("/objects/{id}/activities?limit=3")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.headers()["x-total-count"],
        "7",
        "the header counts everything, not the page"
    );
    let page1: serde_json::Value = res.json().await.unwrap();
    assert_eq!(page1.as_array().unwrap().len(), 3);
    assert_eq!(page1[0]["title"], "Fill 6", "newest first");

    let page3: serde_json::Value = app
        .client
        .get(app.url(&format!("/objects/{id}/activities?limit=3&offset=6")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page3.as_array().unwrap().len(), 1);
    assert_eq!(
        page3[0]["title"], "Fill 0",
        "the last page holds the oldest entry"
    );

    // The total tracks the filter, not the table.
    let res = app
        .client
        .get(app.url(&format!("/objects/{id}/activities?category=repair")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.headers()["x-total-count"], "0");

    // Absurd limits are clamped rather than rejected.
    let res = app
        .client
        .get(app.url(&format!("/objects/{id}/activities?limit=99999")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let all: serde_json::Value = res.json().await.unwrap();
    assert_eq!(all.as_array().unwrap().len(), 7);
}

#[tokio::test]
async fn recent_titles_are_distinct_and_newest_first() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    for (date, title, cost, counter) in [
        ("2026-01-05", "Fuel", 5000, 10_000),
        ("2026-02-05", "Oil change", 9000, 11_000),
        ("2026-03-05", "Fuel", 6210, 12_000),
    ] {
        let res = app
            .client
            .post(app.url(&format!("/objects/{id}/activities")))
            .json(&json!({
                "date": date, "category": "fuel", "title": title,
                "cost_cents": cost, "counter_value": counter
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let out: Vec<serde_json::Value> = app
        .client
        .get(app.url(&format!("/objects/{id}/recent-titles")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(out.len(), 2, "one row per distinct (title, category)");
    assert_eq!(out[0]["title"], "Fuel", "the most recent title comes first");
    assert_eq!(out[0]["last_date"], "2026-03-05");
    assert_eq!(
        out[0]["last_cost_cents"], 6210,
        "the newest occurrence supplies the cost"
    );
    assert_eq!(out[0]["last_counter"], 12_000);
    assert_eq!(out[1]["title"], "Oil change");
}

/// A trip suggestion's `last_from_place`/`last_to_place` come from the same newest occurrence
/// as its `last_cost_cents`/`last_counter` -- see the "Repeat" chip's use of them in
/// `ActivityForm.svelte`. Any other category's suggestion carries neither, exactly as the
/// stored row itself never carries a place outside `trip`.
#[tokio::test]
async fn recent_titles_carry_trip_places_from_the_newest_occurrence() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let id = bike["id"].as_i64().unwrap();

    for (date, from, to, start, end) in [
        ("2026-01-01", "Home", "Office", 0, 10),
        ("2026-02-01", "Office", "Home", 10, 20),
    ] {
        let res = app
            .client
            .post(app.url(&format!("/objects/{id}/activities")))
            .json(&json!({
                "date": date, "category": "trip", "title": "Commute", "notes": "",
                "start_counter": start, "counter_value": end, "from_place": from, "to_place": to
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }
    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({
            "date": "2026-03-01", "category": "maintenance", "title": "Brakes", "notes": ""
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    let out: Vec<serde_json::Value> = app
        .client
        .get(app.url(&format!("/objects/{id}/recent-titles")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let commute = out.iter().find(|s| s["title"] == "Commute").unwrap();
    assert_eq!(
        commute["last_from_place"], "Office",
        "the newest (2026-02-01) trip's own from: {commute}"
    );
    assert_eq!(commute["last_to_place"], "Home");
    let brakes = out.iter().find(|s| s["title"] == "Brakes").unwrap();
    assert_eq!(brakes["last_from_place"], serde_json::Value::Null);
    assert_eq!(brakes["last_to_place"], serde_json::Value::Null);
}

#[tokio::test]
async fn fuel_quantity_round_trips_and_is_validated() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({
            "date": "2026-03-05", "category": "fuel", "title": "Fuel",
            "counter_value": 12_000, "cost_cents": 6210, "quantity_milli": 41_300
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let a: serde_json::Value = res.json().await.unwrap();
    assert_eq!(a["quantity_milli"], 41_300);

    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({
            "date": "2026-03-06", "category": "fuel", "title": "Fuel", "quantity_milli": -1
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400, "a negative quantity is rejected");

    let no_counter = app.create_object(&app.client, "Drill", None).await;
    let nid = no_counter["id"].as_i64().unwrap();
    let res = app
        .client
        .post(app.url(&format!("/objects/{nid}/activities")))
        .json(&json!({
            "date": "2026-03-06", "category": "fuel", "title": "Fuel", "quantity_milli": 1000
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        400,
        "a quantity without a fuel unit cannot become consumption"
    );

    let res = app
        .client
        .post(app.url("/objects"))
        .json(&json!({
            "name": "Electric meter", "type": "appliance", "fuel_unit": "kwh"
        }))
        .send()
        .await
        .unwrap();
    let meter: serde_json::Value = res.json().await.unwrap();
    let mid = meter["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{mid}/activities"))).json(&json!({
        "date": "2026-03-06", "category": "fuel", "title": "Monthly use", "quantity_milli": 312000
    })).send().await.unwrap();
    assert_eq!(
        res.status(),
        201,
        "a resource quantity does not need an unrelated mileage/hour counter"
    );
}

#[tokio::test]
async fn recent_titles_of_another_users_object_are_404() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = anna
        .get(app.url(&format!("/objects/{id}/recent-titles")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn a_replayed_create_returns_the_first_row_instead_of_duplicating_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let body = json!({
        "date": "2026-03-05", "category": "fuel", "title": "Fuel",
        "cost_cents": 6210, "client_op_id": "op-abc-123"
    });

    let first = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 201);
    let first: serde_json::Value = first.json().await.unwrap();

    let again = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 200, "a replay is not a new creation");
    let again: serde_json::Value = again.json().await.unwrap();
    assert_eq!(again["id"], first["id"], "the same row comes back");

    let list: Vec<serde_json::Value> = app
        .client
        .get(app.url(&format!("/objects/{id}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.len(), 1, "the fill-up was logged once");
}

#[tokio::test]
async fn one_client_op_id_cannot_be_reused_across_objects() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let a = app.create_object(&app.client, "Golf", Some("km")).await;
    let b = app.create_object(&app.client, "Bike", Some("km")).await;
    let (aid, bid) = (a["id"].as_i64().unwrap(), b["id"].as_i64().unwrap());
    let body = json!({ "date": "2026-03-05", "category": "other", "title": "X", "client_op_id": "op-dup" });

    assert_eq!(
        app.client
            .post(app.url(&format!("/objects/{aid}/activities")))
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    let res = app
        .client
        .post(app.url(&format!("/objects/{bid}/activities")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        409,
        "the id is the client's promise that this is the same op"
    );
}

/// The unique index on client_op_id is partial (`WHERE client_op_id IS NOT NULL`), because
/// every activity an online client writes leaves the column NULL. If the index treated NULLs
/// as equal, the second of these creates would trip a uniqueness violation.
#[tokio::test]
async fn many_activities_with_no_client_op_id_do_not_conflict() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/activities"));

    for i in 0..5 {
        let body = json!({
            "date": "2026-03-05", "category": "other", "title": format!("No op id {i}"),
            "client_op_id": null
        });
        let res = app.client.post(&base).json(&body).send().await.unwrap();
        assert_eq!(
            res.status(),
            201,
            "explicit null client_op_id must never collide"
        );
    }

    let list: Vec<serde_json::Value> = app
        .client
        .get(&base)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.len(), 5);
}

/// A blank client_op_id must not be treated as a real idempotency key -- a client library
/// that always populates the field (rather than omitting it) would otherwise collapse every
/// blank-id create on an object onto the first one, silently losing the rest.
#[tokio::test]
async fn blank_client_op_id_is_treated_as_absent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/activities"));

    for (i, op_id) in ["", "   "].into_iter().enumerate() {
        let body = json!({
            "date": "2026-03-05", "category": "other", "title": format!("Blank op id {i}"),
            "client_op_id": op_id
        });
        let res = app.client.post(&base).json(&body).send().await.unwrap();
        assert_eq!(
            res.status(),
            201,
            "a blank (or whitespace-only) client_op_id must not block a real create"
        );
    }

    let list: Vec<serde_json::Value> = app
        .client
        .get(&base)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        list.len(),
        2,
        "two blank-id creates on one object must produce two distinct rows, not one"
    );
}

/// Regression test for the migration itself, not just the application-level pre-check: every
/// other test in this file only ever reaches `create`'s SELECT-then-INSERT dance. This goes
/// through the pool directly, so a future migration that dropped or weakened the partial
/// unique index would fail this test even though the pre-check alone would still look fine.
#[tokio::test]
async fn the_partial_unique_index_rejects_a_duplicate_non_null_op_id() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let insert = || {
        sqlx::query(
            // `client_uuid` is supplied even though the column is nullable and this test does
            // not read it: every writer in the application sets one, and a row without it is
            // one the sync protocol cannot name in `changes` or in a `field_clock` sweep. A
            // fixture that manufactures the impossible row makes the suite disagree with the
            // system it is testing.
            "INSERT INTO activities (object_id, date, category, title, notes, client_op_id, client_uuid, created_at, updated_at) \
             VALUES ($1, '2026-03-05', 'other', 'Raw insert', '', 'raw-dup', $2, '2026-03-05T00:00:00Z', '2026-03-05T00:00:00Z')",
        )
        .bind(id)
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&app.state.db)
    };

    insert().await.unwrap();
    let err = insert().await.unwrap_err();
    let db_err = err
        .as_database_error()
        .expect("a constraint violation carries a database error");
    assert!(db_err.is_unique_violation(), "{db_err}");
}

/// The pre-check in `create` is SELECT-then-INSERT, so concurrent requests carrying one op id
/// all miss it and race the INSERT; the losers land on the partial unique index and must adopt
/// the winner's row rather than 500. Proven here by genuinely concurrent requests, where the
/// tests above only ever exercise the sequential path that the pre-check itself handles.
#[tokio::test]
async fn concurrent_creates_sharing_one_op_id_resolve_to_one_activity() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/activities"));

    let mut tasks = Vec::new();
    for i in 0..6 {
        let (client, base) = (app.client.clone(), base.clone());
        tasks.push(tokio::spawn(async move {
            client
                .post(&base)
                .json(&serde_json::json!({
                    "date": "2026-03-05",
                    "category": "maintenance",
                    // Deliberately different bodies: whichever wins, every caller must be told about
                    // the one row that exists, not about the body it happened to send.
                    "title": format!("Oil change {i}"),
                    "client_op_id": "op-shared",
                }))
                .send()
                .await
                .unwrap()
        }));
    }

    let mut ids = Vec::new();
    for t in tasks {
        let res = t.await.unwrap();
        assert!(
            res.status() == 200 || res.status() == 201,
            "{}",
            res.status()
        );
        let a: serde_json::Value = res.json().await.unwrap();
        ids.push(a["id"].as_i64().unwrap());
    }
    assert!(
        ids.windows(2).all(|w| w[0] == w[1]),
        "one op id, one activity: {ids:?}"
    );

    let list: Vec<serde_json::Value> = app
        .client
        .get(&base)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
}

/// The client pages the timeline in chunks sized against this cap (`MAX_LIMIT` in
/// frontend/src/lib/timeline-load.ts). The server CLAMPS a larger limit rather than refusing
/// it, so if the two numbers ever drift apart the client asks for more than it gets, believes
/// it received a full window, and silently drops every row past the cap -- with a 200 and
/// nothing to notice. This pins the behaviour; `the_client_and_server_agree_on_the_page_limit`
/// below pins that the two constants are the same number.
#[tokio::test]
async fn the_page_limit_is_capped_at_500_and_the_cap_is_silent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    for i in 0..3 {
        app.client
            .post(app.url(&format!("/objects/{id}/activities")))
            .json(
                &json!({ "date": "2026-01-01", "category": "fuel", "title": format!("Entry {i}") }),
            )
            .send()
            .await
            .unwrap();
    }

    // Asking for more than the cap is accepted, not rejected: that is exactly what makes a
    // drift silent, and what the client's chunking exists to work around.
    let res = app
        .client
        .get(app.url(&format!("/objects/{id}/activities?limit=100000")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers().get("x-total-count").unwrap(), "3");

    // The documented cap, asked for exactly, is honoured.
    let res = app
        .client
        .get(app.url(&format!("/objects/{id}/activities?limit=500")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "500 is within the cap");
}

/// The other half of the contract above: the client hard-codes the same cap, because it has to
/// size its own chunks against it and the server offers no way to ask. A comment asking the
/// next person to keep the two in step is not a check; reading the other file is.
///
/// Deliberately on this side of the wire: the frontend's tsconfig keeps Node's types out of
/// browser code, and widening it so a test could read a file would let `process` and friends
/// type-check inside the app itself.
#[test]
fn the_client_and_server_agree_on_the_page_limit() {
    let ts = std::fs::read_to_string("frontend/src/lib/timeline-load.ts")
        .expect("frontend/src/lib/timeline-load.ts should be readable from the crate root");
    let declared = ts
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("export const MAX_LIMIT = ")?
                .strip_suffix(';')
        })
        .expect(
            "MAX_LIMIT is no longer declared in timeline-load.ts the way this test looks for it",
        )
        .parse::<i64>()
        .expect("MAX_LIMIT should be a plain number");

    assert_eq!(
        declared, 500,
        "the client's MAX_LIMIT and the server's must match, or the client silently loses \
         every row past the smaller of the two",
    );
}

/// `reminders.done_activity_id ... ON DELETE SET NULL` cannot fire for the tombstoning
/// UPDATE that replaced the hard DELETE, so the activity delete handler has to clear it by
/// hand. Without that, a reminder marked done by an activity that is later deleted would keep
/// pointing at an id `GET /activities/{id}` now answers 404 for.
#[tokio::test]
async fn deleting_the_activity_that_completed_a_reminder_clears_its_done_activity_id() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&act(
            "2026-01-01",
            "maintenance",
            Some(100_000),
            Some(5_000),
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let activity: serde_json::Value = res.json().await.unwrap();
    let aid = activity["id"].as_i64().unwrap();

    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Oil", "due_counter": 105_000 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let reminder: serde_json::Value = res.json().await.unwrap();
    let rid = reminder["id"].as_i64().unwrap();

    let res = app
        .client
        .post(app.url(&format!("/reminders/{rid}/done")))
        .json(&json!({ "activity_id": aid }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let done: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        done["done"]["done_activity_id"], aid,
        "sanity: the reminder really points at it"
    );

    assert_eq!(
        app.client
            .delete(app.url(&format!("/activities/{aid}")))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    let res = app
        .client
        .get(app.url(&format!("/reminders/{rid}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "the reminder itself must still read");
    let r: serde_json::Value = res.json().await.unwrap();
    assert!(
        r["done_activity_id"].is_null(),
        "the stale pointer must be cleared: {r}"
    );
    assert!(
        r["done_at"].is_string(),
        "clearing the pointer must not undo done-ness"
    );
}

#[tokio::test]
async fn an_activity_create_honours_and_replays_on_client_uuid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let url = app.url(&format!("/objects/{}/activities", car["id"]));
    let body = json!({ "date": "2026-09-01", "category": "repair", "title": "Wipers", "client_uuid": "phone-0002-wipers" });
    let res = app.client.post(&url).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 201);
    let first: serde_json::Value = res.json().await.unwrap();
    assert_eq!(first["client_uuid"], "phone-0002-wipers");
    let res = app.client.post(&url).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let second: serde_json::Value = res.json().await.unwrap();
    assert_eq!(first["id"], second["id"]);
    let boot: serde_json::Value = app
        .client
        .get(app.url("/sync/bootstrap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let a = boot["activities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == first["id"])
        .unwrap();
    assert_eq!(a["client_uuid"], "phone-0002-wipers");
}

#[tokio::test]
async fn an_activity_client_uuid_cannot_adopt_a_row_on_another_object() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let golf = app.create_object(&app.client, "Golf", Some("km")).await;
    let bike = app.create_object(&app.client, "Bike", None).await;
    let body = json!({ "date": "2026-09-01", "category": "repair", "title": "Wipers", "client_uuid": "phone-0002-wipers" });
    app.client
        .post(app.url(&format!("/objects/{}/activities", golf["id"])))
        .json(&body)
        .send()
        .await
        .unwrap();
    let res = app
        .client
        .post(app.url(&format!("/objects/{}/activities", bike["id"])))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        409,
        "the uuid names a row under a different object"
    );
}

/// The `title` filter is a trimmed, case-insensitive exact match -- the same fold `last_done`
/// groups by (`fold_title` in `src/api/activities.rs`), so a title tapped there and a filter
/// applied here always agree.
#[tokio::test]
async fn title_filter_matches_trimmed_case_insensitively_and_combines_with_category() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/activities"));
    for (date, category, title) in [
        ("2026-01-10", "maintenance", "Bremsbeläge vorne"),
        ("2026-05-12", "repair", "Bremsbeläge vorne"),
        ("2026-02-01", "repair", "Kette"),
    ] {
        let res = app
            .client
            .post(&base)
            .json(&json!({ "date": date, "category": category, "title": title, "notes": "" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    // `Url::parse` (inside `reqwest::Client::get`) percent-encodes a raw space and non-ASCII
    // bytes embedded in the query string on its own, so the literal accented, spaced text below
    // reaches the server exactly as typed.
    let res = app
        .client
        .get(format!("{base}?title=BREMSBELÄGE VORNE"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["x-total-count"], "2");
    let rows: Vec<serde_json::Value> = res.json().await.unwrap();
    let titles: Vec<&str> = rows.iter().map(|a| a["title"].as_str().unwrap()).collect();
    assert_eq!(
        titles,
        ["Bremsbeläge vorne", "Bremsbeläge vorne"],
        "both, and only, the brake pad entries"
    );

    // Combined with category, the filter narrows further.
    let res = app
        .client
        .get(format!("{base}?title= bremsbeläge vorne &category=repair"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.headers()["x-total-count"], "1");
    let rows: Vec<serde_json::Value> = res.json().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["date"], "2026-05-12");

    // A title matching nothing is an empty page, not every entry.
    let res = app
        .client
        .get(format!("{base}?title=Nonexistent"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.headers()["x-total-count"], "0");
    let rows: Vec<serde_json::Value> = res.json().await.unwrap();
    assert!(rows.is_empty());
}
