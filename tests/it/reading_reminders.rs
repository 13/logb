//! Reading reminders: "log the odometer every month". Due when the newest reading is older than
//! the interval, satisfied by any entry that carries a counter value, never marked done.

mod common;
use axum::routing::post;
use axum::Router;
use chrono::{Days, Months, NaiveDate};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

fn today() -> NaiveDate {
    chrono::Utc::now().date_naive()
}

async fn add_activity(
    app: &common::TestApp,
    object_id: i64,
    body: serde_json::Value,
) -> serde_json::Value {
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{body} -- {}", res.text().await.unwrap());
    res.json().await.unwrap()
}

async fn reading_reminder(
    app: &common::TestApp,
    object_id: i64,
    body: serde_json::Value,
) -> serde_json::Value {
    let res = app
        .client
        .post(app.url(&format!("/objects/{object_id}/reminders")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{body} -- {}", res.text().await.unwrap());
    res.json().await.unwrap()
}

async fn get(app: &common::TestApp, path: &str) -> serde_json::Value {
    app.get_json(path).await
}

async fn car(app: &common::TestApp) -> i64 {
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn fixed_reading_schedules_follow_readings_and_restore_after_deletion() {
    let app = common::spawn().await;
    let id = car(&app).await;
    for schedule in ["daily", "weekly:1", "monthly:15", "monthly:last", "yearly:2:29"] {
        let r = reading_reminder(&app, id, json!({
            "title": schedule, "kind": "reading", "schedule": schedule, "due_date": "2020-01-01"
        })).await;
        assert_eq!(r["due"], true);
        let entry = add_activity(&app, id, json!({ "category": "reading", "title": "Counter", "date": today().to_string(), "counter_value": 100 })).await;
        let path = format!("/reminders/{}", r["id"]);
        let advanced = get(&app, &path).await;
        assert_eq!(advanced["due"], false, "{schedule}: {advanced}");
        let expected = logb::domain::reminder::CalendarSchedule::parse(schedule).unwrap().next_after(today()).unwrap().to_string();
        assert_eq!(advanced["next_due_date"], expected);
        let removed = app.client.delete(app.url(&format!("/activities/{}", entry["id"]))).send().await.unwrap();
        assert!(removed.status().is_success());
        assert_eq!(get(&app, &path).await["due"], true);
    }
}

#[tokio::test]
async fn a_reading_reminder_is_validated_as_its_own_kind() {
    let app = common::spawn().await;
    let id = car(&app).await;
    let home = app.create_object(&app.client, "Home", None).await["id"]
        .as_i64()
        .unwrap();

    for (object, body) in [
        (
            home,
            json!({ "title": "Meter", "kind": "reading", "every_n": 1, "every_unit": "month" }),
        ),
        (id, json!({ "title": "Km", "kind": "reading" })),
        (
            id,
            json!({ "title": "Km", "kind": "reading", "every_n": 0, "every_unit": "month" }),
        ),
        (
            id,
            json!({ "title": "Km", "kind": "reading", "every_n": 1, "every_unit": "day" }),
        ),
        (
            id,
            json!({ "title": "Km", "kind": "reading", "every_n": 1, "every_unit": "month", "due_counter": 5 }),
        ),
        (
            id,
            json!({ "title": "Km", "kind": "reading", "every_n": 1, "every_unit": "month", "repeat_months": 1 }),
        ),
        (
            id,
            json!({ "title": "Oil", "due_date": "2030-01-01", "every_n": 1, "every_unit": "month", "schedule": "daily" }),
        ),
        (
            id,
            json!({ "title": "Km", "kind": "odometer", "every_n": 1, "every_unit": "month" }),
        ),
    ] {
        let res = app
            .client
            .post(app.url(&format!("/objects/{object}/reminders")))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 400, "{body}");
    }

    // Leaving the start out means "from today".
    let r = reading_reminder(
        &app,
        id,
        json!({ "title": "Km", "kind": "reading", "every_n": 1, "every_unit": "month" }),
    )
    .await;
    assert_eq!(r["kind"], "reading");
    assert_eq!(r["due_date"], today().to_string());
    assert_eq!(r["due"], true, "no reading yet, and the start is today");

    // A reminder does not change kind.
    let res = app
        .client
        .patch(app.url(&format!("/reminders/{}", r["id"])))
        .json(&json!({ "title": "Km", "due_date": "2030-01-01" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    // ...but its interval does.
    let res = app.client.patch(app.url(&format!("/reminders/{}", r["id"])))
        .json(&json!({ "title": "Km", "kind": "reading", "due_date": r["due_date"], "every_n": 2, "every_unit": "week" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let r: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        (r["every_n"].as_i64(), r["every_unit"].as_str()),
        (Some(2), Some("week"))
    );
}

#[tokio::test]
async fn any_entry_with_a_counter_clears_it_and_deleting_that_entry_brings_it_back() {
    let app = common::spawn().await;
    let id = car(&app).await;
    let r = reading_reminder(&app, id, json!({
        "title": "Log km", "kind": "reading", "every_n": 1, "every_unit": "month", "due_date": "2020-01-01"
    })).await;
    let rid = r["id"].as_i64().unwrap();
    assert_eq!(r["due"], true);
    assert_eq!(
        get(&app, &format!("/objects/{id}")).await["stats"]["due_reminder_count"],
        1
    );

    // An entry without a counter says nothing about the odometer.
    add_activity(
        &app,
        id,
        json!({ "date": today().to_string(), "category": "repair", "title": "Wipers" }),
    )
    .await;
    assert_eq!(get(&app, &format!("/reminders/{rid}")).await["due"], true);

    // A fuel stop that notes the odometer is a reading, wherever it was logged from.
    let fuel = add_activity(&app, id, json!({
        "date": today().to_string(), "category": "fuel", "title": "Fuel", "counter_value": 42_000, "quantity_milli": 40_000
    })).await;
    let r = get(&app, &format!("/reminders/{rid}")).await;
    assert_eq!(r["due"], false);
    assert_eq!(r["last_reading_date"], today().to_string());
    assert_eq!(
        r["next_due_date"],
        today()
            .checked_add_months(Months::new(1))
            .unwrap()
            .to_string()
    );
    assert_eq!(
        get(&app, &format!("/objects/{id}")).await["stats"]["due_reminder_count"],
        0
    );
    let due: Vec<serde_json::Value> =
        serde_json::from_value(get(&app, "/reminders/due").await).unwrap();
    assert!(due.is_empty(), "{due:#?}");
    assert_eq!(
        get(&app, &format!("/objects/{id}")).await["stats"]["last_reading_date"],
        today().to_string()
    );

    let res = app
        .client
        .delete(app.url(&format!("/activities/{}", fuel["id"])))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(
        get(&app, &format!("/reminders/{rid}")).await["due"],
        true,
        "the reading it rested on is gone"
    );
    assert_eq!(
        get(&app, &format!("/objects/{id}")).await["stats"]["due_reminder_count"],
        1
    );
}

#[tokio::test]
async fn an_old_reading_does_not_count_and_a_future_dated_one_is_ignored() {
    let app = common::spawn().await;
    let id = car(&app).await;
    let r = reading_reminder(&app, id, json!({
        "title": "Log km", "kind": "reading", "every_n": 1, "every_unit": "month", "due_date": "2020-01-01"
    })).await;
    let rid = r["id"].as_i64().unwrap();

    let two_months_ago = today().checked_sub_months(Months::new(2)).unwrap();
    add_activity(&app, id, json!({ "date": two_months_ago.to_string(), "category": "reading", "title": "Odometer", "counter_value": 40_000 })).await;
    assert_eq!(
        get(&app, &format!("/reminders/{rid}")).await["due"],
        true,
        "a month has passed since"
    );

    // A typo'd year must not silence the reminder for years.
    let future = today().checked_add_days(Days::new(400)).unwrap();
    add_activity(&app, id, json!({ "date": future.to_string(), "category": "reading", "title": "Odometer", "counter_value": 41_000 })).await;
    let r = get(&app, &format!("/reminders/{rid}")).await;
    assert_eq!(r["due"], true);
    assert_eq!(r["last_reading_date"], two_months_ago.to_string());

    // A reading dated tomorrow is a device past midnight while the server's timezone is not,
    // and it counts: the person just did what the reminder asked.
    let tomorrow = today().checked_add_days(Days::new(1)).unwrap();
    add_activity(&app, id, json!({ "date": tomorrow.to_string(), "category": "reading", "title": "Odometer", "counter_value": 40_500 })).await;
    let r = get(&app, &format!("/reminders/{rid}")).await;
    assert_eq!(r["due"], false);
    assert_eq!(r["last_reading_date"], tomorrow.to_string());
    assert_eq!(
        get(&app, &format!("/objects/{id}")).await["stats"]["due_reminder_count"],
        0
    );
}

#[tokio::test]
async fn a_reading_is_logged_with_a_value_and_its_reminder_is_never_marked_done() {
    let app = common::spawn().await;
    let id = car(&app).await;
    let res = app
        .client
        .post(app.url(&format!("/objects/{id}/activities")))
        .json(&json!({ "date": today().to_string(), "category": "reading", "title": "Odometer" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400, "a reading with no value");

    let r = reading_reminder(
        &app,
        id,
        json!({ "title": "Log km", "kind": "reading", "every_n": 1, "every_unit": "month" }),
    )
    .await;
    let res = app
        .client
        .post(app.url(&format!("/reminders/{}/done", r["id"])))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 409);
}

#[tokio::test]
async fn skipping_a_due_reading_hides_it_until_the_snooze_lapses() {
    let app = common::spawn().await;
    let id = car(&app).await;
    let r = reading_reminder(&app, id, json!({
        "title": "Log km", "kind": "reading", "every_n": 1, "every_unit": "month", "due_date": "2020-01-01"
    })).await;
    let rid = r["id"].as_i64().unwrap();

    let res = app
        .client
        .post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 30 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let r: serde_json::Value = res.json().await.unwrap();
    assert_eq!(r["due"], false);
    // Overdue, so the skip runs from today -- not from 2020.
    assert_eq!(
        r["snoozed_until"],
        today().checked_add_days(Days::new(30)).unwrap().to_string()
    );
    assert_eq!(
        get(&app, &format!("/objects/{id}")).await["stats"]["due_reminder_count"],
        0
    );
}

#[tokio::test]
async fn usage_estimates_when_a_mileage_service_comes_due() {
    let app = common::spawn().await;
    let id = car(&app).await;
    // 3_000 km over 60 days: 50 km a day.
    let start = today().checked_sub_days(Days::new(60)).unwrap();
    add_activity(&app, id, json!({ "date": start.to_string(), "category": "reading", "title": "Odometer", "counter_value": 10_000 })).await;
    add_activity(&app, id, json!({ "date": today().to_string(), "category": "reading", "title": "Odometer", "counter_value": 13_000 })).await;

    let insights = get(&app, &format!("/objects/{id}/insights")).await;
    assert_eq!(insights["counter_per_day_milli"], 50_000);
    let months = insights["usage_by_month"].as_array().unwrap();
    assert_eq!(months.len(), 12);
    let this_month = months.last().unwrap();
    assert_eq!(this_month["month"], today().format("%Y-%m").to_string());
    assert_eq!(
        this_month["amount"], 3_000,
        "the reading 60 days ago is in an earlier month, so all of it lands here"
    );
    assert!(
        insights["by_category"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["bucket"] != "reading"),
        "readings carry no cost and have no place in the cost breakdown"
    );

    let service = app
        .client
        .post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Service", "due_counter": 14_000 }))
        .send()
        .await
        .unwrap();
    assert_eq!(service.status(), 201);
    let service: serde_json::Value = service.json().await.unwrap();
    let expected = today().checked_add_days(Days::new(20)).unwrap().to_string();
    assert_eq!(service["estimated_due_date"], expected);
    assert_eq!(
        service["due"], false,
        "an estimate never makes a reminder due"
    );

    let far = app
        .client
        .post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Timing belt", "due_counter": 90_000 }))
        .send()
        .await
        .unwrap();
    let far: serde_json::Value = far.json().await.unwrap();

    let soon: Vec<serde_json::Value> =
        serde_json::from_value(get(&app, "/reminders/due?within_days=30").await).unwrap();
    let ids: Vec<&serde_json::Value> = soon.iter().map(|r| &r["id"]).collect();
    assert!(
        ids.contains(&&service["id"]),
        "a mileage-only service now shows up as coming up: {soon:#?}"
    );
    assert!(!ids.contains(&&far["id"]), "4 years out is not coming up");
    assert!(
        serde_json::from_value::<Vec<serde_json::Value>>(get(&app, "/reminders/due").await)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_reading_reminder_survives_export_and_import() {
    let app = common::spawn().await;
    let id = car(&app).await;
    reading_reminder(&app, id, json!({
        "title": "Log km", "kind": "reading", "every_n": 2, "every_unit": "week", "due_date": "2026-01-05"
    })).await;
    reading_reminder(&app, id, json!({
        "title": "Calendar km", "kind": "reading", "schedule": "monthly:last", "due_date": "2026-01-05"
    })).await;
    add_activity(&app, id, json!({ "date": "2026-01-01", "category": "reading", "title": "Odometer", "counter_value": 1_000 })).await;

    let zip = app
        .client
        .get(app.url("/export"))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap()
        .to_vec();
    let anna = app.create_user_client("anna", "password123").await;
    let res = anna
        .post(app.url("/import"))
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objs: Vec<serde_json::Value> = anna
        .get(app.url("/objects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let nid = objs[0]["id"].as_i64().unwrap();
    let rems: Vec<serde_json::Value> = anna
        .get(app.url(&format!("/objects/{nid}/reminders")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rems.len(), 2);
    let fixed = rems.iter().find(|r| r["schedule"] == "monthly:last").unwrap();
    assert_eq!(fixed["kind"], "reading");
    assert_eq!(fixed["due_date"], "2026-01-05");
    let rems: Vec<_> = rems.iter().filter(|r| r["schedule"].is_null()).collect();
    assert_eq!(rems[0]["kind"], "reading");
    assert_eq!(rems[0]["every_n"], 2);
    assert_eq!(rems[0]["every_unit"], "week");
    assert_eq!(rems[0]["due_date"], "2026-01-05");
    let acts: serde_json::Value = anna
        .get(app.url(&format!("/objects/{nid}/activities")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(acts[0]["category"], "reading");
}

/// Records the `Title` and `Click` headers and the body of every POST it receives.
#[derive(Clone, Default)]
struct Inbox(Arc<Mutex<Vec<(String, String, String)>>>);

async fn webhook() -> (String, Inbox) {
    let inbox = Inbox::default();
    let sink = inbox.clone();
    let app = Router::new().route(
        "/hook",
        post(move |headers: axum::http::HeaderMap, body: String| {
            let sink = sink.clone();
            async move {
                let header = |k: &str| {
                    headers
                        .get(k)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string()
                };
                sink.0
                    .lock()
                    .unwrap()
                    .push((header("title"), header("click"), body));
                "ok"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://{addr}/hook"), inbox)
}

#[tokio::test]
async fn the_digest_lists_readings_under_their_own_heading_with_a_link() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| {
        c.notify_url = Some(url);
        c.notify_format = "text".into();
        c.public_url = Some("https://logb.example/".into());
    })
    .await;
    let id = car(&app).await;
    reading_reminder(&app, id, json!({
        "title": "Log km", "kind": "reading", "every_n": 1, "every_unit": "month", "due_date": "2020-01-01"
    })).await;

    logb::notify::tick(&app.state, 9)
        .await
        .unwrap()
        .expect("a digest was due");
    let got = inbox.0.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    let link = format!("https://logb.example/objects/{id}/reading");
    assert_eq!(got[0].0, "LogB: 1 reminder due");
    assert_eq!(
        got[0].1, link,
        "one reminder: tapping the notification opens its form"
    );
    assert_eq!(got[0].2, format!("Readings to log:\nGolf: Log km {link}"));
}

#[tokio::test]
async fn services_come_first_and_readings_follow() {
    let app = common::spawn().await;
    let id = car(&app).await;
    app.client
        .post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Oil change", "due_date": "2020-01-01" }))
        .send()
        .await
        .unwrap();
    reading_reminder(&app, id, json!({
        "title": "Log km", "kind": "reading", "every_n": 1, "every_unit": "month", "due_date": "2020-01-01"
    })).await;

    let digest = logb::notify::collect(&app.state).await.unwrap().unwrap();
    assert_eq!(
        digest.message,
        "Golf: Oil change\n\nReadings to log:\nGolf: Log km"
    );
    let kinds: Vec<&str> = digest.reminders.iter().map(|r| r.kind.as_str()).collect();
    assert!(kinds.contains(&"reading") && kinds.contains(&"service"));
    assert!(
        digest.reminders.iter().all(|r| r.link.is_none()),
        "no public URL, no links"
    );
}
