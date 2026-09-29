use crate::common;
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

async fn costed(app: &common::TestApp, id: i64, date: NaiveDate, cost: i64, counter: Option<i64>) -> i64 {
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": date.to_string(), "category": "maintenance", "title": "m", "notes": "",
        "cost_cents": cost, "counter_value": counter
    })).send().await.unwrap();
    assert_eq!(res.status(), 201);
    res.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
}

async fn child(app: &common::TestApp, name: &str, parent: i64, archived: bool) -> i64 {
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": name, "type": "car", "counter_unit": "km", "parent_id": parent, "archived": archived
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
}

/// Pins every derived stats field, and that the list (in each of its shapes) and the single
/// read agree on them, so the aggregate behind them can be rewritten without the numbers moving.
#[tokio::test]
async fn every_list_shape_and_the_read_agree_on_the_derived_stats() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let t = today();
    let golf = app.create_object(&app.client, "Golf", Some("km")).await["id"].as_i64().unwrap();
    costed(&app, golf, t - Duration::days(20), 1000, Some(100)).await;
    costed(&app, golf, t - Duration::days(5), 500, Some(250)).await;
    let gone = costed(&app, golf, t - Duration::days(1), 9999, Some(99_999)).await;
    let res = app.client.delete(app.url(&format!("/activities/{gone}"))).send().await.unwrap();
    assert_eq!(res.status(), 204);
    costed(&app, golf, t + Duration::days(30), 200, None).await;
    for body in [json!({"title": "Due", "due_counter": 200}), json!({"title": "Later", "due_counter": 900}),
                 json!({"title": "Future", "due_date": (t + Duration::days(9)).to_string()})] {
        let res = app.client.post(app.url(&format!("/objects/{golf}/reminders"))).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 201);
    }
    let trailer = child(&app, "Trailer", golf, false).await;
    costed(&app, trailer, t - Duration::days(2), 300, None).await;
    let old = child(&app, "Old", golf, true).await;
    costed(&app, old, t - Duration::days(400), 700, Some(7)).await;
    let empty = app.create_object(&app.client, "Empty", None).await["id"].as_i64().unwrap();

    let golf_stats = json!({
        "total_cost_cents": 1700, "activity_count": 3, "current_counter": 250,
        "latest_weight_grams": null, "latest_weight_date": null, "due_reminder_count": 1,
        "last_reading_date": (t - Duration::days(5)).to_string(),
        "last_activity_date": (t - Duration::days(5)).to_string(),
    });
    let trailer_stats = json!({
        "total_cost_cents": 300, "activity_count": 1, "current_counter": null,
        "latest_weight_grams": null, "latest_weight_date": null, "due_reminder_count": 0,
        "last_reading_date": null, "last_activity_date": (t - Duration::days(2)).to_string(),
        "counter_per_day_milli": null,
    });
    let old_stats = json!({
        "total_cost_cents": 700, "activity_count": 1, "current_counter": 7,
        "latest_weight_grams": null, "latest_weight_date": null, "due_reminder_count": 0,
        "last_reading_date": (t - Duration::days(400)).to_string(),
        "last_activity_date": (t - Duration::days(400)).to_string(),
        "counter_per_day_milli": null,
    });
    let empty_stats = json!({
        "total_cost_cents": 0, "activity_count": 0, "current_counter": null,
        "latest_weight_grams": null, "latest_weight_date": null, "due_reminder_count": 0,
        "last_reading_date": null, "last_activity_date": null, "counter_per_day_milli": null,
    });
    let check = |object: &Value, want: &Value| {
        for (k, v) in want.as_object().unwrap() {
            assert_eq!(&object["stats"][k], v, "{k} of {}", object["name"]);
        }
    };

    let all = app.get_json("/objects?all=true").await;
    assert_eq!(all.as_array().unwrap().len(), 3, "{all}");
    check(&find(&all, golf), &golf_stats);
    check(&find(&all, trailer), &trailer_stats);
    check(&find(&all, empty), &empty_stats);
    let roots = app.get_json("/objects").await;
    assert_eq!(roots.as_array().unwrap().len(), 2, "{roots}");
    check(&find(&roots, golf), &golf_stats);
    let children = app.get_json(&format!("/objects?parent_id={golf}")).await;
    assert_eq!(children.as_array().unwrap().len(), 1, "{children}");
    check(&find(&children, trailer), &trailer_stats);
    let archived = app.get_json("/objects?archived=true&all=true").await;
    assert_eq!(archived.as_array().unwrap().len(), 1, "{archived}");
    check(&find(&archived, old), &old_stats);

    for (id, listed) in [(golf, &all), (trailer, &all), (empty, &all), (old, &archived)] {
        let read = app.get_json(&format!("/objects/{id}")).await;
        assert_eq!(read["stats"], find(listed, id)["stats"], "read and list disagree for {id}");
        assert_eq!(read["cover_file_id"], find(listed, id)["cover_file_id"]);
    }
}
