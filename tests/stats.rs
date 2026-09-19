mod common;
use chrono::{Months, NaiveDate};
use serde_json::{json, Value};

async fn object(app: &common::TestApp, body: Value) -> i64 {
    let res = app.client.post(app.url("/objects")).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
}

async fn energy_entry(app: &common::TestApp, object_id: i64, date: String, quantity_milli: i64) {
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({ "date": date, "category": "fuel", "title": "Charge", "quantity_milli": quantity_milli, "counter_value": 1 }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
}

#[tokio::test]
async fn household_energy_groups_kwh_by_month() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let today: NaiveDate = logb::db::today().parse().unwrap();
    let current = today.format("%Y-%m-%d").to_string();
    let previous = today.checked_sub_months(Months::new(1)).unwrap().format("%Y-%m-%d").to_string();
    let kwh = object(&app, json!({ "name": "Bike", "type": "e_bike", "counter_unit": "km", "fuel_unit": "kwh" })).await;
    let home = object(&app, json!({ "name": "Home battery", "type": "appliance", "fuel_unit": "kwh", "counter_unit": "h" })).await;
    let petrol = object(&app, json!({ "name": "Car", "type": "car", "fuel_unit": "l", "counter_unit": "km" })).await;
    energy_entry(&app, kwh, current.clone(), 200_000).await;
    energy_entry(&app, home, current.clone(), 112_000).await;
    energy_entry(&app, kwh, previous, 300_000).await;
    energy_entry(&app, petrol, current, 999_000).await;
    let future = (today + chrono::Days::new(2)).format("%Y-%m-%d").to_string();
    energy_entry(&app, kwh, future, 500_000).await;

    let out = app.get_json("/stats/energy").await;
    assert_eq!(out["current_kwh_milli"], 312_000);
    assert_eq!(out["previous_kwh_milli"], 300_000);
    assert_eq!(out["months"].as_array().unwrap().last().unwrap()["charges"], 2);
    assert_eq!(out["months"].as_array().unwrap().last().unwrap()["objects"], 2);
}

#[tokio::test]
async fn household_fuel_keeps_litres_and_gallons_separate() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let today: NaiveDate = logb::db::today().parse().unwrap();
    let date = today.format("%Y-%m-%d").to_string();
    let litres = object(&app, json!({ "name": "Oil tank", "type": "home", "fuel_unit": "l", "counter_unit": "h" })).await;
    let gallons = object(&app, json!({ "name": "Generator", "type": "other", "fuel_unit": "gal", "counter_unit": "h" })).await;
    energy_entry(&app, litres, date.clone(), 125_500).await;
    energy_entry(&app, gallons, date, 12_250).await;
    let out = app.get_json("/stats/fuel").await;
    assert_eq!(out["current_liters_milli"], 125_500);
    assert_eq!(out["current_gallons_milli"], 12_250);
    assert_eq!(out["months"].as_array().unwrap().last().unwrap()["charges"], 2);
}

async fn cost(app: &common::TestApp, object_id: i64, date: &str, category: &str, cents: i64) -> i64 {
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({ "date": date, "category": category, "title": category, "notes": "", "cost_cents": cents }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
}

fn buckets(v: &Value) -> Vec<(String, i64)> {
    v.as_array().unwrap().iter()
        .map(|b| (b["bucket"].as_str().unwrap().to_string(), b["cost_cents"].as_i64().unwrap()))
        .collect()
}

/// House (bought 2024 for 3,000.00) > Boiler; a car. Costs in 2025 and 2026.
async fn seed(app: &common::TestApp) -> (i64, i64, i64) {
    let house = object(app, json!({ "name": "House", "type": "home", "description": "",
        "purchase_date": "2024-05-01", "purchase_price_cents": 300_000 })).await;
    let boiler = object(app, json!({ "name": "Boiler", "type": "appliance", "description": "", "parent_id": house })).await;
    let car = object(app, json!({ "name": "Car", "type": "car", "counter_unit": "km", "description": "" })).await;
    cost(app, house, "2025-03-10", "repair", 100_000).await;
    cost(app, boiler, "2026-02-01", "maintenance", 25_000).await;
    cost(app, car, "2026-06-01", "fuel", 8_000).await;
    cost(app, car, "2026-07-01", "repair", 42_000).await;
    (house, boiler, car)
}

#[tokio::test]
async fn all_years_roll_children_into_their_parent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;

    let out = app.get_json("/stats").await;
    assert_eq!(out["total_cents"], 175_000);
    assert_eq!(out["years"], json!(["2026", "2025"]));
    assert_eq!(buckets(&out["over_time"]), [("2025".into(), 100_000), ("2026".into(), 75_000)]);
    let house = &out["by_object"][0];
    assert_eq!(house["name"], "House");
    assert_eq!(house["type"], "home");
    assert_eq!(house["cost_cents"], 125_000);
    assert_eq!(house["children"][0]["name"], "Boiler");
    assert_eq!(out["by_object"][1]["cost_cents"], 50_000);
    assert_eq!(buckets(&out["by_type"]), [("home".into(), 100_000), ("car".into(), 50_000), ("appliance".into(), 25_000)]);
}

#[tokio::test]
async fn a_year_gives_twelve_months_and_keeps_the_full_year_list() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;

    let out = app.get_json("/stats?year=2026").await;
    assert_eq!(out["total_cents"], 75_000);
    assert_eq!(out["over_time"].as_array().unwrap().len(), 12);
    assert_eq!(out["over_time"][1], json!({ "bucket": "2026-02", "cost_cents": 25_000 }));
    assert_eq!(out["over_time"][0], json!({ "bucket": "2026-01", "cost_cents": 0 }));
    assert_eq!(out["years"], json!(["2026", "2025"]));
    assert_eq!(buckets(&out["by_category"]), [("repair".into(), 42_000), ("maintenance".into(), 25_000), ("fuel".into(), 8_000)]);
}

#[tokio::test]
async fn purchases_add_their_own_row_unless_a_purchase_entry_already_has_the_cost() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_, _, car) = seed(&app).await;

    let out = app.get_json("/stats?purchases=true").await;
    assert_eq!(out["total_cents"], 475_000);
    assert_eq!(out["years"], json!(["2026", "2025", "2024"]));
    assert!(buckets(&out["by_category"]).contains(&("purchase_price".into(), 300_000)));

    // A car whose purchase is logged as an entry: its price field must not count again.
    let res = app.client.patch(app.url(&format!("/objects/{car}")))
        .json(&json!({ "name": "Car", "type": "car", "counter_unit": "km", "description": "", "purchase_price_cents": 900_000 }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    cost(&app, car, "2023-01-15", "purchase", 900_000).await;
    let out = app.get_json("/stats?purchases=true").await;
    assert_eq!(out["total_cents"], 475_000 + 900_000);
    assert!(buckets(&out["by_category"]).contains(&("purchase".into(), 900_000)));
    assert!(buckets(&out["by_category"]).contains(&("purchase_price".into(), 300_000)), "only the house's price");
}

#[tokio::test]
async fn deleted_rows_are_gone_archived_objects_stay() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_, _, car) = seed(&app).await;

    let fuel = app.get_json(&format!("/objects/{car}/activities")).await;
    let fuel_id = fuel.as_array().unwrap().iter().find(|a| a["category"] == "fuel").unwrap()["id"].as_i64().unwrap();
    assert!(app.client.delete(app.url(&format!("/activities/{fuel_id}"))).send().await.unwrap().status().is_success());
    let res = app.client.patch(app.url(&format!("/objects/{car}")))
        .json(&json!({ "name": "Car", "type": "car", "counter_unit": "km", "description": "", "archived": true }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let out = app.get_json("/stats").await;
    assert_eq!(out["total_cents"], 167_000);
    let car_row = out["by_object"].as_array().unwrap().iter().find(|o| o["name"] == "Car").unwrap().clone();
    assert_eq!(car_row["archived"], true);
    assert_eq!(car_row["cost_cents"], 42_000);

    let house = out["by_object"][0]["id"].as_i64().unwrap();
    assert!(app.client.delete(app.url(&format!("/objects/{house}"))).send().await.unwrap().status().is_success());
    let out = app.get_json("/stats").await;
    assert_eq!(out["total_cents"], 42_000, "a deleted house takes its boiler with it");
}

#[tokio::test]
async fn another_users_spend_never_appears() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;
    let anna = app.create_user_client("anna", "password123").await;
    let out: Value = anna.get(app.url("/stats")).send().await.unwrap().json().await.unwrap();
    assert_eq!(out["total_cents"], 0);
    assert!(out["by_object"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_bad_year_is_400() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for q in ["year=abc", "year=-1", "year=10000"] {
        let res = app.client.get(app.url(&format!("/stats?{q}"))).send().await.unwrap();
        assert_eq!(res.status(), 400, "{q}");
    }
}

#[tokio::test]
async fn the_edges_of_the_year_range_are_200() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for q in ["year=0", "year=9999"] {
        let res = app.client.get(app.url(&format!("/stats?{q}"))).send().await.unwrap();
        assert_eq!(res.status(), 200, "{q}");
    }
}

#[tokio::test]
async fn a_years_purchase_price_lands_in_its_own_month() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;

    let out = app.get_json("/stats?year=2024&purchases=true").await;
    assert_eq!(out["total_cents"], 300_000, "the house's purchase price only");
    assert_eq!(out["over_time"].as_array().unwrap().len(), 12);
    assert_eq!(out["over_time"][4], json!({ "bucket": "2024-05", "cost_cents": 300_000 }));
    assert_eq!(out["years"], json!(["2026", "2025", "2024"]));
}

#[tokio::test]
async fn by_type_buckets_carry_custom_keys() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let scooter = app.post_json("/types", &json!({ "name": "E-scooter", "icon": "e-bike", "categories": ["repair"] })).await;
    let key = scooter["key"].as_str().unwrap().to_string();
    let kick = object(&app, json!({ "name": "Kick", "type": key, "description": "" })).await;
    cost(&app, kick, "2026-03-01", "repair", 4_200).await;
    let out = app.get_json("/stats").await;
    assert_eq!(buckets(&out["by_type"]), [(key, 4_200)]);
}
