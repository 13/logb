mod common;
use serde_json::json;

#[tokio::test]
async fn appearance_and_delivery_hour_are_validated_and_private() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert!(app.get_json("/me/appearance").await.is_null());
    let appearance = json!({"locale":"de", "theme":"dark", "dateFormat":"dmy-dot", "firstDayOfWeek":"sunday"});
    let response = app.client.put(app.url("/me/appearance")).json(&appearance).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(app.get_json("/me/appearance").await, appearance);
    let mut invalid = appearance.clone(); invalid["firstDayOfWeek"] = json!("friday");
    assert_eq!(app.client.put(app.url("/me/appearance")).json(&invalid).send().await.unwrap().status(), 400);
    assert_eq!(app.get_json("/me/appearance").await, appearance);
    assert_eq!(app.client.put(app.url("/me/notifications/hour")).json(&json!({"hour":25})).send().await.unwrap().status(), 400);
    assert_eq!(app.client.put(app.url("/me/notifications/hour")).json(&json!({"hour":17})).send().await.unwrap().status(), 200);
    assert_eq!(app.get_json("/me/notifications").await["hour"], 17);
    app.client.post(app.url("/users")).json(&json!({"username":"other", "password":"correct horse"})).send().await.unwrap();
    let other = reqwest::Client::builder().cookie_store(true).build().unwrap();
    assert_eq!(other.post(app.url("/auth/login")).json(&json!({"username":"other", "password":"correct horse"})).send().await.unwrap().status(), 200);
    let theirs: serde_json::Value = other.get(app.url("/me/appearance")).send().await.unwrap().json().await.unwrap();
    assert!(theirs.is_null());
    let notification: serde_json::Value = other.get(app.url("/me/notifications")).send().await.unwrap().json().await.unwrap();
    assert!(notification["deliveries"].as_array().unwrap().is_empty());
    assert_ne!(notification["hour"], 17);
}

/// The one-day reading horizon moves with the user's zone: a reading dated the user's
/// tomorrow counts as the latest, the user's day after tomorrow does not.
#[tokio::test]
async fn the_reading_horizon_follows_the_users_zone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({ "hour": 8, "timezone": "Pacific/Kiritimati" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let user_today = chrono::Utc::now().with_timezone(&chrono_tz::Pacific::Kiritimati).date_naive();
    let tomorrow = user_today.succ_opt().unwrap();
    let after = tomorrow.succ_opt().unwrap();
    for (date, counter) in [(tomorrow, 1_000), (after, 2_000)] {
        let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
            .json(&json!({ "date": date.to_string(), "category": "maintenance", "title": "reading", "counter_value": counter }))
            .send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }
    let object = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(object["stats"]["last_reading_date"], tomorrow.to_string(), "{object}");
}

/// West of the instance the user's today can still be the instance's yesterday, so a reminder
/// due on the instance's today is not yet due for them.
#[tokio::test]
async fn a_user_in_a_far_western_zone_sees_the_instances_today_as_tomorrow() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let instance_today = chrono::Utc::now().date_naive();
    let user_today = chrono::Utc::now().with_timezone(&chrono_tz::Pacific::Pago_Pago).date_naive();
    let res = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Inspection", "due_date": instance_today.to_string() }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let res = app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({ "hour": 8, "timezone": "Pacific/Pago_Pago" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let due = app.get_json("/reminders/due").await;
    // Before 11:00 UTC the user is still on yesterday and the reminder is a day away.
    let expected = usize::from(instance_today <= user_today);
    assert_eq!(due.as_array().unwrap().len(), expected, "{due}");
    let all = app.get_json(&format!("/objects/{id}/reminders")).await;
    let expected_days = (instance_today - user_today).num_days();
    assert_eq!(all[0]["days_until"], expected_days, "{all}");
}

/// `AuthUser` carries the zone on every request, so a handler can read the user's today
/// without a second query. Proven through `/reminders/due` and the object's due count.
#[tokio::test]
async fn a_user_in_a_far_eastern_zone_sees_the_instances_tomorrow_as_due() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    // Instance is UTC (the harness sets no timezone). Kiritimati is UTC+14, so from 10:00 UTC
    // onward it is already tomorrow there; before that the two agree and this test would be
    // vacuous, so the due date is chosen from the user's own today instead of "UTC + 1".
    let user_today = chrono::Utc::now().with_timezone(&chrono_tz::Pacific::Kiritimati).date_naive();
    let instance_today = chrono::Utc::now().date_naive();
    let res = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Inspection", "due_date": user_today.to_string() }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);

    let res = app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({ "hour": 8, "timezone": "Pacific/Kiritimati" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let due = app.get_json("/reminders/due").await;
    assert_eq!(due.as_array().unwrap().len(), 1, "due for the user in UTC+14: {due}");
    assert_eq!(due[0]["days_until"], 0);
    let object = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(object["stats"]["due_reminder_count"], 1);

    // Back on the instance zone the same reminder is only due once the instance's date
    // catches up -- which it already has whenever the two calendars agree right now.
    let res = app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({ "hour": 8, "timezone": null })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let due = app.get_json("/reminders/due").await;
    let expected = usize::from(user_today <= instance_today);
    assert_eq!(due.as_array().unwrap().len(), expected, "due on the instance zone: {due}");
}
