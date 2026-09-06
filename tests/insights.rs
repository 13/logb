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
        // A fuel row with a counter reading but no recorded quantity: it has a cost and a
        // counter (so it does move the overall counter_span and cost_per_counter_milli), but
        // it is not a usable fill -- it must not move fuel.per_100_milli or
        // fuel.cost_per_counter_milli, which only look at fills with both fields set.
        ("2026-04-01", "fuel", "Fuel (no quantity)", 9_999, 10_950, None),
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
    assert_eq!(years[0]["cost_cents"], 47_999);
    assert_eq!(years[0]["count"], 4);
    assert_eq!(years[1]["bucket"], "2025");
    assert_eq!(years[1]["cost_cents"], 5_000);

    let cats = out["by_category"].as_array().unwrap();
    let repair = cats.iter().find(|c| c["bucket"] == "repair").unwrap();
    assert_eq!(repair["cost_cents"], 30_000);

    // The overall span and cost cover every activity, including the fuel row that has no
    // quantity and the repair -- 10_000 to 10_950, 52_999 cents in total.
    assert_eq!(out["counter_span"]["from"], 10_000);
    assert_eq!(out["counter_span"]["to"], 10_950);
    // 52_999 cents over 950 km
    assert_eq!(out["cost_per_counter_milli"], 55_788);
    assert_eq!(out["fuel"]["unit"], "l");
    assert_eq!(out["fuel"]["quantity_milli"], 85_000);
    // 40 L over 800 km -- the fuel span, not the overall 950 km span above, and the fuel
    // row with no quantity does not count as a fill.
    assert_eq!(out["fuel"]["per_100_milli"], 5_000);
    // The fuel block now describes exactly that same 800 km window: the first fill's 5_000
    // cents are excluded just like its quantity, leaving 4_000 + 4_000 = 8_000 cents over
    // 800 km. The 9_999-cent fuel row without a quantity does not move this either, even
    // though it does have a counter reading and does move the overall figures above.
    assert_eq!(out["fuel"]["cost_per_counter_milli"], 10_000);
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
