mod common;
use serde_json::json;

async fn add_activity(app: &common::TestApp, object_id: i64, date: &str, counter: Option<i64>) -> serde_json::Value {
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({ "date": date, "category": "maintenance", "title": "service", "counter_value": counter }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    res.json().await.unwrap()
}

#[tokio::test]
async fn due_by_date_and_counter_and_repeat() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/reminders"));

    let res = app.client.post(&base).json(&json!({ "title": "Oil", "due_date": "2000-01-01", "due_counter": 110_000, "repeat_months": 12, "repeat_counter": 15_000 })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let oil: serde_json::Value = res.json().await.unwrap();
    assert_eq!(oil["due"], true, "date in the past");
    let res = app.client.post(&base).json(&json!({ "title": "Tyres", "due_counter": 105_000 })).send().await.unwrap();
    let tyres: serde_json::Value = res.json().await.unwrap();
    assert_eq!(tyres["due"], false, "no counter reading yet");
    let res = app.client.post(&base).json(&json!({ "title": "TÜV", "due_date": "2999-01-01" })).send().await.unwrap();
    assert_eq!(res.status(), 201);

    add_activity(&app, id, "2026-01-01", Some(106_000)).await;
    let due: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due")).send().await.unwrap().json().await.unwrap();
    let titles: Vec<&str> = due.iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Oil", "Tyres"]);
    assert_eq!(due[0]["object_name"], "Golf");
    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(obj["stats"]["due_reminder_count"], 2);

    let service = add_activity(&app, id, "2026-02-01", Some(107_000)).await;
    let res = app.client.post(app.url(&format!("/reminders/{}/done", oil["id"]))).json(&json!({ "activity_id": service["id"] })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let done: serde_json::Value = res.json().await.unwrap();
    assert!(done["done"]["done_at"].is_string());
    assert_eq!(done["done"]["done_activity_id"], service["id"]);
    assert_eq!(done["next"]["due_date"], "2027-02-01");
    assert_eq!(done["next"]["due_counter"], 122_000);
    assert_eq!(done["next"]["repeat_months"], 12);

    let res = app.client.post(app.url(&format!("/reminders/{}/done", tyres["id"]))).json(&json!({})).send().await.unwrap();
    let done: serde_json::Value = res.json().await.unwrap();
    assert!(done["next"].is_null(), "no repeat");

    let all: Vec<serde_json::Value> = app.client.get(&base).send().await.unwrap().json().await.unwrap();
    assert_eq!(all.len(), 4);
    let due: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due")).send().await.unwrap().json().await.unwrap();
    assert!(due.is_empty());
}

#[tokio::test]
async fn crud_validation_and_isolation() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let home = app.create_object(&app.client, "Home", None).await;
    let hid = home["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{hid}/reminders"));

    for body in [
        json!({ "title": "x" }),
        json!({ "title": " ", "due_date": "2030-01-01" }),
        json!({ "title": "x", "due_date": "2030/01/01" }),
        json!({ "title": "x", "due_counter": 5 }),
        json!({ "title": "x", "due_date": "2030-01-01", "repeat_months": 0 }),
    ] {
        assert_eq!(app.client.post(&base).json(&body).send().await.unwrap().status(), 400, "{body}");
    }

    let r: serde_json::Value = app.client.post(&base).json(&json!({ "title": "Chimney", "due_date": "2030-01-01" })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    let res = app.client.patch(app.url(&format!("/reminders/{rid}"))).json(&json!({ "title": "Chimney sweep", "notes": "call", "due_date": "2030-02-01" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let r: serde_json::Value = res.json().await.unwrap();
    assert_eq!(r["title"], "Chimney sweep");

    assert_eq!(anna.get(&base).send().await.unwrap().status(), 404);
    assert_eq!(anna.get(app.url(&format!("/reminders/{rid}"))).send().await.unwrap().status(), 404);
    assert_eq!(anna.post(app.url(&format!("/reminders/{rid}/done"))).json(&json!({})).send().await.unwrap().status(), 404);
    assert_eq!(anna.delete(app.url(&format!("/reminders/{rid}"))).send().await.unwrap().status(), 404);
    assert_eq!(app.client.delete(app.url(&format!("/reminders/{rid}"))).send().await.unwrap().status(), 204);
}

/// Completing a reminder twice is a conflict, not a second completion.
#[tokio::test]
async fn a_reminder_cannot_be_completed_twice() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Oil", "due_date": "2020-01-01" }))
        .send().await.unwrap().json().await.unwrap();
    let done_url = app.url(&format!("/reminders/{}/done", r["id"]));

    assert_eq!(app.client.post(&done_url).json(&json!({})).send().await.unwrap().status(), 200);
    let res = app.client.post(&done_url).json(&json!({})).send().await.unwrap();
    assert_eq!(res.status(), 409, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "conflict");
}

#[tokio::test]
async fn due_list_can_look_ahead() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let soon = (chrono::Utc::now().date_naive() + chrono::Duration::days(10)).to_string();
    let far = (chrono::Utc::now().date_naive() + chrono::Duration::days(90)).to_string();

    for (title, date) in [("Soon", &soon), ("Far", &far)] {
        let res = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
            "title": title, "notes": "", "due_date": date
        })).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let now: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due"))
        .send().await.unwrap().json().await.unwrap();
    assert!(now.is_empty(), "nothing is due yet, and the default must not change");

    let ahead: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due?within_days=30"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(ahead.len(), 1, "only the reminder inside the window");
    assert_eq!(ahead[0]["title"], "Soon");
    assert_eq!(ahead[0]["due"], false);
    assert_eq!(ahead[0]["days_until"], 10);
}

#[tokio::test]
async fn snooze_suppresses_an_overdue_reminder_without_rewriting_its_due_date() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap();
    let r: serde_json::Value = res.json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    assert_eq!(r["due"], true);

    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert_eq!(out["due"], false, "a snoozed reminder is no longer due");
    let expected = (chrono::Utc::now().date_naive() + chrono::Duration::days(7)).to_string();
    // Snooze suppresses; it does not rewrite the real due date.
    assert_eq!(out["due_date"], "2020-01-01", "snooze must not touch the real due_date");
    assert_eq!(out["snoozed_until"], expected);
    assert!(out["days_until"].as_i64().unwrap() < 0, "days_until stays truthful about the real due date");

    for bad in [json!({ "days": 0 }), json!({ "days": 400 })] {
        let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
            .json(&bad).send().await.unwrap();
        assert_eq!(res.status(), 400, "{bad}");
    }
}

/// Snooze's whole reason to exist: a counter-due reminder (no useful due_date) is not
/// suppressed by rewriting a date nobody looks at. It must actually stop being due, and
/// resume being due once the snooze lapses.
#[tokio::test]
async fn snooze_suppresses_a_counter_due_reminder_and_lapses() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    add_activity(&app, id, "2026-01-01", Some(60_000)).await;

    let res = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Service", "due_counter": 60_000 }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let r: serde_json::Value = res.json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    assert_eq!(r["due"], true, "the counter has already been reached");
    assert!(r["due_date"].is_null());

    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert_eq!(out["due"], false, "snoozing must actually suppress a counter-due reminder");
    assert!(out["due_date"].is_null(), "snooze does not invent a due_date");
    assert_eq!(out["due_counter"], 60_000, "snooze does not touch due_counter either");

    let due: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due?within_days=30"))
        .send().await.unwrap().json().await.unwrap();
    assert!(due.iter().all(|x| x["id"] != rid), "a snoozed reminder must not appear in the lookahead either");

    // Lapse the snooze by writing an already-past date directly through the pool, the way
    // the domain-level tests cover "on or before today resumes normal rules" -- there is no
    // time-travel helper in this test harness, so this is the integration-level equivalent.
    sqlx::query("UPDATE reminders SET snoozed_until = '2020-01-01' WHERE id = ?")
        .bind(rid)
        .execute(&app.state.db)
        .await
        .unwrap();

    let res = app.client.get(app.url(&format!("/reminders/{rid}"))).send().await.unwrap();
    let out: serde_json::Value = res.json().await.unwrap();
    assert_eq!(out["due"], true, "a lapsed snooze suppresses nothing");
}

/// A snoozed reminder must not show up in the lookahead window at all, regardless of how far
/// ahead the caller asks.
#[tokio::test]
async fn a_snoozed_reminder_is_absent_from_the_lookahead() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 30 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let due: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due?within_days=30"))
        .send().await.unwrap().json().await.unwrap();
    assert!(due.iter().all(|x| x["id"] != rid), "a snoozed reminder must not appear, even in the lookahead");
}

/// Un-snoozing makes a suppressed reminder due again immediately, without touching the real
/// `due_date` -- clearing `snoozed_until` is the only thing this route does.
#[tokio::test]
async fn unsnoozing_makes_a_suppressed_reminder_due_again() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let snoozed: serde_json::Value = res.json().await.unwrap();
    assert_eq!(snoozed["due"], false, "snooze must have suppressed it first");

    let res = app.client.delete(app.url(&format!("/reminders/{rid}/snooze"))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert!(out["snoozed_until"].is_null(), "un-snoozing clears snoozed_until");
    assert_eq!(out["due"], true, "clearing the snooze must make it due again immediately");
    assert_eq!(out["due_date"], "2020-01-01", "un-snoozing must not touch the real due_date");

    let due: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due"))
        .send().await.unwrap().json().await.unwrap();
    assert!(due.iter().any(|x| x["id"] == rid), "the reminder must be counted as due again");
}

/// Un-snoozing a reminder that was never snoozed is a no-op success: the caller asked for
/// "not snoozed", and that state already holds, so there is nothing to reject.
#[tokio::test]
async fn unsnoozing_a_never_snoozed_reminder_changes_nothing() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    assert!(r["snoozed_until"].is_null());

    let res = app.client.delete(app.url(&format!("/reminders/{rid}/snooze"))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert!(out["snoozed_until"].is_null());
    assert_eq!(out["due"], true);
    assert_eq!(out["due_date"], "2020-01-01");
}

/// Another user's reminder is a 404, same as every other reminder route.
#[tokio::test]
async fn unsnoozing_another_users_reminder_is_not_found() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();

    let res = anna.delete(app.url(&format!("/reminders/{rid}/snooze"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}

/// The dashboard's `stats.due_reminder_count` has its own hand-written SQL encoding of
/// dueness (see src/api/objects.rs), so un-snoozing must be checked against it explicitly
/// rather than assumed to agree with `is_due`.
#[tokio::test]
async fn unsnoozing_makes_the_object_count_it_as_due_again() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();

    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(obj["stats"]["due_reminder_count"], 1, "a past due_date counts as due");

    app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(obj["stats"]["due_reminder_count"], 0, "a snoozed reminder must not count as due");

    let res = app.client.delete(app.url(&format!("/reminders/{rid}/snooze"))).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(obj["stats"]["due_reminder_count"], 1, "un-snoozing must make it count as due again");
}

/// `unsnooze`'s handler comment argues that clearing `snoozed_until` on an already-done
/// reminder is harmless -- `done_at.is_some()` always wins in `ReminderOut::from`, so there is
/// nothing left to guard against -- but, unlike `snooze` (see `a_done_reminder_cannot_be_snoozed`
/// below), that claim had no test at all. This pins both halves of it down: the call succeeds
/// (no 409, unlike snooze) rather than being rejected, and the reminder stays not-due
/// afterwards precisely because it is done, not because of anything unsnooze itself does.
#[tokio::test]
async fn unsnoozing_a_done_reminder_succeeds_and_it_stays_not_due() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();

    // Snooze it, then mark it done while still snoozed -- `done` does not check `snoozed_until`
    // -- so the row unsnooze sees below carries both `done_at` and a live `snoozed_until`.
    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let res = app.client.post(app.url(&format!("/reminders/{rid}/done"))).json(&json!({})).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let res = app.client.delete(app.url(&format!("/reminders/{rid}/snooze"))).send().await.unwrap();
    assert_eq!(res.status(), 200, "unsnoozing a done reminder must succeed, not 409 like snooze does: {}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert!(out["snoozed_until"].is_null(), "snoozed_until is still cleared for a done reminder");
    assert!(out["done_at"].is_string());
    assert_eq!(out["due"], false, "a done reminder must stay not-due even after unsnoozing it");
}

#[tokio::test]
async fn a_done_reminder_cannot_be_snoozed() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    assert_eq!(app.client.post(app.url(&format!("/reminders/{rid}/done"))).json(&json!({}))
        .send().await.unwrap().status(), 200);
    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 409);
}

/// The lookahead sibling of `a_snoozed_reminder_is_absent_from_the_lookahead`, which only ever
/// exercised a reminder whose due date was already PAST -- such a reminder is excluded by its
/// negative `days_until` alone, so that test passes even when the snooze is ignored entirely.
/// A reminder due in the future is the case where the lookahead arm actually runs, and where
/// snoozing must still take it off the dashboard.
#[tokio::test]
async fn snoozing_a_future_reminder_takes_it_out_of_the_lookahead_too() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let due = chrono::Utc::now().date_naive() + chrono::Duration::days(5);
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Inspection", "notes": "", "due_date": due.to_string()
    })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();

    let listed: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due?within_days=30"))
        .send().await.unwrap().json().await.unwrap();
    assert!(listed.iter().any(|x| x["id"] == rid), "it must be in the lookahead before the snooze");

    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert_eq!(out["due"], false);
    assert!(out["days_until"].as_i64().unwrap() > 0, "the real due date is still ahead");

    let listed: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due?within_days=30"))
        .send().await.unwrap().json().await.unwrap();
    assert!(
        listed.iter().all(|x| x["id"] != rid),
        "a snoozed reminder must leave the lookahead, not just the due list",
    );

    // ...and come back once the snooze lapses, so this suppresses rather than deletes.
    sqlx::query("UPDATE reminders SET snoozed_until = '2020-01-01' WHERE id = ?")
        .bind(rid).execute(&app.state.db).await.unwrap();
    let listed: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due?within_days=30"))
        .send().await.unwrap().json().await.unwrap();
    assert!(listed.iter().any(|x| x["id"] == rid), "a lapsed snooze suppresses nothing");
}
