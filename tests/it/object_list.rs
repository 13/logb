mod common;
use chrono::{Duration, NaiveDate};
use serde_json::{json, Value};

fn today() -> NaiveDate {
    NaiveDate::parse_from_str(&logb::db::today(), "%Y-%m-%d").unwrap()
}

async fn entry(app: &common::TestApp, id: i64, date: NaiveDate, category: &str, counter: Option<i64>) {
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": date.to_string(), "category": category, "title": category, "notes": "", "counter_value": counter
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
}

fn find(list: &Value, id: i64) -> Value {
    list.as_array().unwrap().iter().find(|o| o["id"] == id).expect("object in list").clone()
}

#[tokio::test]
async fn stats_carry_last_activity_and_the_usage_rate() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let t = today();
    // 2,700 km over the 90 days between the readings: 30 km a day.
    entry(&app, id, t - Duration::days(100), "reading", Some(10_000)).await;
    entry(&app, id, t - Duration::days(10), "reading", Some(12_700)).await;
    entry(&app, id, t - Duration::days(3), "maintenance", None).await;
    // A planned expense next month is not recent activity.
    entry(&app, id, t + Duration::days(30), "maintenance", None).await;

    let list = app.get_json("/objects?all=true").await;
    let listed = find(&list, id);
    assert_eq!(listed["stats"]["last_activity_date"], (t - Duration::days(3)).to_string());
    assert_eq!(listed["stats"]["counter_per_day_milli"], 30_000);

    let read = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(read["stats"]["last_activity_date"], listed["stats"]["last_activity_date"]);
    assert_eq!(read["stats"]["counter_per_day_milli"], 30_000);
}

#[tokio::test]
async fn an_object_without_entries_has_neither() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    assert!(car["stats"]["last_activity_date"].is_null());
    assert!(car["stats"]["counter_per_day_milli"].is_null());
    let id = car["id"].as_i64().unwrap();
    let listed = find(&app.get_json("/objects?all=true").await, id);
    assert!(listed["stats"]["last_activity_date"].is_null());
    assert!(listed["stats"]["counter_per_day_milli"].is_null());
}

#[tokio::test]
async fn another_users_readings_never_set_my_rate() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let mine = app.create_object(&app.client, "Mine", Some("km")).await;
    let theirs = app.create_object(&anna, "Theirs", Some("km")).await;
    let t = today();
    for (date, counter) in [(t - Duration::days(100), 0), (t, 5_000)] {
        let res = anna.post(app.url(&format!("/objects/{}/activities", theirs["id"]))).json(&json!({
            "date": date.to_string(), "category": "reading", "title": "r", "notes": "", "counter_value": counter
        })).send().await.unwrap();
        assert_eq!(res.status(), 201);
    }
    let listed = find(&app.get_json("/objects?all=true").await, mine["id"].as_i64().unwrap());
    assert!(listed["stats"]["counter_per_day_milli"].is_null());
    assert_eq!(app.get_json("/objects?all=true").await.as_array().unwrap().len(), 1);
}
