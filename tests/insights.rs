mod common;
use serde_json::json;

#[tokio::test]
async fn insights_roll_up_cost_and_consumption() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    for (date, category, title, cost, counter, qty) in [
        ("2025-06-01", "fuel", "Fuel", 5_000, 10_000, Some(45_000)),
        ("2026-01-10", "fuel", "Fuel", 4_000, 10_400, Some(20_000)),
        ("2026-02-10", "fuel", "Fuel", 4_000, 10_800, Some(20_000)),
        ("2026-03-10", "repair", "Brakes", 30_000, 10_900, None),
    ] {
        let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
            "date": date, "category": category, "title": title,
            "cost_cents": cost, "counter_value": counter, "quantity_milli": qty
        })).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let out: serde_json::Value = app.client
        .get(app.url(&format!("/objects/{id}/insights")))
        .send().await.unwrap().json().await.unwrap();

    let years = out["by_year"].as_array().unwrap();
    assert_eq!(years[0]["bucket"], "2026", "newest year first");
    assert_eq!(years[0]["cost_cents"], 38_000);
    assert_eq!(years[0]["count"], 3);
    assert_eq!(years[1]["bucket"], "2025");
    assert_eq!(years[1]["cost_cents"], 5_000);

    let cats = out["by_category"].as_array().unwrap();
    let repair = cats.iter().find(|c| c["bucket"] == "repair").unwrap();
    assert_eq!(repair["cost_cents"], 30_000);

    assert_eq!(out["counter_span"]["from"], 10_000);
    assert_eq!(out["counter_span"]["to"], 10_900);
    // 43_000 cents over 900 km
    assert_eq!(out["cost_per_counter_milli"], 47_777);
    assert_eq!(out["fuel"]["unit"], "l");
    assert_eq!(out["fuel"]["quantity_milli"], 85_000);
    // 40 L over 800 km
    assert_eq!(out["fuel"]["per_100_milli"], 5_000);
}

#[tokio::test]
async fn insights_of_an_empty_object_are_all_null() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let out: serde_json::Value = app.client
        .get(app.url(&format!("/objects/{id}/insights")))
        .send().await.unwrap().json().await.unwrap();
    assert!(out["by_year"].as_array().unwrap().is_empty());
    assert!(out["cost_per_counter_milli"].is_null());
    assert!(out["counter_span"].is_null());
    assert!(out["fuel"].is_null());
}

#[tokio::test]
async fn insights_of_another_users_object_are_404() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = anna.get(app.url(&format!("/objects/{id}/insights"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}
