mod common;
use chrono::{Duration, NaiveDate};
use serde_json::json;

/// The reading form only needs how fast the counter rises; this endpoint answers that without
/// the cost rollups `/insights` computes around it, and must agree with `/insights` on the number.
#[tokio::test]
async fn usage_reports_the_same_rate_insights_does() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let today = NaiveDate::parse_from_str(&logb::db::today(), "%Y-%m-%d").unwrap();
    for (date, counter) in [(today - Duration::days(100), 10_000), (today, 13_000)] {
        let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
            "date": date.to_string(), "category": "reading", "title": "Reading", "notes": "", "counter_value": counter
        })).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let usage = app.get_json(&format!("/objects/{id}/usage")).await;
    // 3,000 km over 100 days: 30 km a day, scaled by 1000.
    assert_eq!(usage, json!({ "counter_per_day_milli": 30_000 }));
    let insights = app.get_json(&format!("/objects/{id}/insights")).await;
    assert_eq!(usage["counter_per_day_milli"], insights["counter_per_day_milli"]);
}

#[tokio::test]
async fn usage_is_null_without_enough_readings() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    assert_eq!(app.get_json(&format!("/objects/{id}/usage")).await, json!({ "counter_per_day_milli": null }));
}

#[tokio::test]
async fn usage_of_another_users_object_is_404() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = anna.get(app.url(&format!("/objects/{id}/usage"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}
