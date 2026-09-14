mod common;
use chrono::NaiveDate;
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

async fn object(app: &common::TestApp, body: serde_json::Value) -> i64 {
    let res = app.client.post(app.url("/objects")).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap()
}

async fn entry(app: &common::TestApp, object_id: i64, body: serde_json::Value) {
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities"))).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
}

#[tokio::test]
async fn contents_add_every_descendants_costs_but_leave_counter_figures_alone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = object(&app, json!({ "name": "House", "type": "home", "description": "",
        "purchase_date": "2024-05-01", "purchase_price_cents": 300_000 })).await;
    let boiler = object(&app, json!({ "name": "Boiler", "type": "appliance", "description": "", "parent_id": house,
        "purchase_date": "2025-01-01", "purchase_price_cents": 50_000 })).await;
    let bulb = object(&app, json!({ "name": "Bulb", "type": "appliance", "description": "", "parent_id": boiler })).await;
    let today = logb::db::today();
    entry(&app, house, json!({ "date": "2025-03-10", "category": "repair", "title": "Roof", "notes": "", "cost_cents": 100_000 })).await;
    entry(&app, boiler, json!({ "date": "2026-02-01", "category": "maintenance", "title": "Service", "notes": "", "cost_cents": 25_000 })).await;
    entry(&app, bulb, json!({ "date": today, "category": "repair", "title": "Swap", "notes": "", "cost_cents": 3_000 })).await;

    let own = app.get_json(&format!("/objects/{house}/insights")).await;
    assert_eq!(own["has_contents"], true);
    assert_eq!(own["ownership"]["total_cents"], 400_000);
    assert_eq!(own["ownership"]["purchase_cents"], 300_000);
    assert_eq!(own["ownership"]["since"], "2024-05-01");
    let days = (NaiveDate::parse_from_str(&today, "%Y-%m-%d").unwrap() - NaiveDate::from_ymd_opt(2024, 5, 1).unwrap()).num_days();
    assert_eq!(own["ownership"]["per_year_cents"], 400_000 * 365 / days);
    let months = own["by_month"].as_array().unwrap();
    assert_eq!(months.len(), 12);
    assert_eq!(months[11], json!({ "bucket": &today[..7], "cost_cents": 0 }), "the bulb is not the house's own");
    assert_eq!(own["by_year"].as_array().unwrap().len(), 1, "only the house's own 2025");

    let all = app.get_json(&format!("/objects/{house}/insights?contents=true")).await;
    assert_eq!(all["ownership"]["total_cents"], 478_000);
    assert_eq!(all["ownership"]["purchase_cents"], 350_000);
    assert_eq!(all["ownership"]["since"], "2024-05-01", "since stays the house's own");
    assert_eq!(all["by_month"][11]["cost_cents"], 3_000);
    let cats = all["by_category"].as_array().unwrap();
    assert!(cats.iter().any(|c| c["bucket"] == "maintenance" && c["cost_cents"] == 25_000), "{cats:?}");
    for field in ["counter_span", "cost_per_counter_milli", "fuel", "counter_per_day_milli", "usage_by_month"] {
        assert_eq!(all[field], own[field], "{field} must ignore contents");
    }

    let leaf = app.get_json(&format!("/objects/{bulb}/insights")).await;
    assert_eq!(leaf["has_contents"], false);
}

#[tokio::test]
async fn an_archived_object_is_measured_to_its_archive_date_and_deleted_contents_drop_out() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = object(&app, json!({ "name": "House", "type": "home", "description": "",
        "purchase_date": "2024-01-01", "purchase_price_cents": 100_000 })).await;
    let shed = object(&app, json!({ "name": "Shed", "type": "appliance", "description": "", "parent_id": house })).await;
    let grandchild = object(&app, json!({ "name": "Mower", "type": "appliance", "description": "", "parent_id": shed })).await;
    entry(&app, shed, json!({ "date": "2025-03-10", "category": "repair", "title": "Fix", "notes": "", "cost_cents": 20_000 })).await;
    entry(&app, grandchild, json!({ "date": "2025-03-10", "category": "repair", "title": "Fix", "notes": "", "cost_cents": 5_000 })).await;

    let res = app.client.delete(app.url(&format!("/objects/{grandchild}"))).send().await.unwrap();
    assert_eq!(res.status(), 204, "delete grandchild failed: {}", res.text().await.unwrap());

    let all = app.get_json(&format!("/objects/{house}/insights?contents=true")).await;
    assert_eq!(all["ownership"]["total_cents"], 120_000, "house price + shed repair, grandchild excluded");
    let cats = all["by_category"].as_array().unwrap();
    let repair = cats.iter().find(|c| c["bucket"] == "repair").unwrap();
    assert_eq!(repair["cost_cents"], 20_000, "the deleted grandchild's repair must not be counted");

    // PATCH replaces the object, so every field is resent or the price is lost.
    let res = app.client.patch(app.url(&format!("/objects/{house}"))).json(&json!({
        "name": "House", "type": "home", "description": "",
        "purchase_date": "2024-01-01", "purchase_price_cents": 100_000, "archived": true
    })).send().await.unwrap();
    assert_eq!(res.status(), 200, "archive failed: {}", res.text().await.unwrap());

    let read = app.get_json(&format!("/objects/{house}")).await;
    let archived_at = read["archived_at"].as_str().expect("archived_at is set");
    let archive_day = &archived_at[..10];
    assert_eq!(archive_day, logb::db::today(), "archiving happens today");

    // Archiving happened today, so `until` = today either way -- that cannot tell the archived
    // branch apart from the unarchived one. Backdating the row directly is the only way to prove
    // ownership is measured to the archive date rather than to today.
    sqlx::query("UPDATE objects SET archived_at = '2025-01-01T00:00:00Z' WHERE id = $1")
        .bind(house)
        .execute(&app.state.db)
        .await
        .unwrap();

    let all = app.get_json(&format!("/objects/{house}/insights?contents=true")).await;
    // 2024-01-01 to 2025-01-01 is 366 days (2024 is a leap year).
    assert_eq!(all["ownership"]["per_year_cents"], 120_000 * 365 / 366);
}

#[tokio::test]
async fn fills_draw_a_trend_and_a_purchase_entry_is_the_purchase() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = object(&app, json!({ "name": "Car", "type": "car", "counter_unit": "km", "description": "",
        "purchase_price_cents": 900_000 })).await;
    entry(&app, car, json!({ "date": "2023-01-15", "category": "purchase", "title": "Bought", "notes": "", "cost_cents": 900_000 })).await;
    for (date, counter, qty) in [("2026-01-01", 10_000, 40_000), ("2026-02-01", 10_500, 30_000), ("2026-03-01", 11_000, 25_000)] {
        entry(&app, car, json!({ "date": date, "category": "fuel", "title": "Fuel", "notes": "",
            "counter_value": counter, "quantity_milli": qty })).await;
    }

    let out = app.get_json(&format!("/objects/{car}/insights?contents=true")).await;
    assert_eq!(out["has_contents"], false);
    assert_eq!(out["ownership"]["purchase_cents"], 0, "the purchase entry already counts it");
    assert_eq!(out["ownership"]["total_cents"], 900_000);
    assert!(out["ownership"]["per_year_cents"].is_null(), "created today: under 90 days owned");
    assert_eq!(out["fuel"]["fills"], json!([
        { "date": "2026-02-01", "per_100_milli": 6_000 },
        { "date": "2026-03-01", "per_100_milli": 5_000 },
    ]));
}

#[tokio::test]
async fn contents_must_be_a_boolean_and_another_users_object_stays_hidden() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = object(&app, json!({ "name": "House", "type": "home", "description": "" })).await;
    let res = app.client.get(app.url(&format!("/objects/{house}/insights?contents=maybe"))).send().await.unwrap();
    assert_eq!(res.status(), 400);
    let anna = app.create_user_client("anna", "password123").await;
    let res = anna.get(app.url(&format!("/objects/{house}/insights?contents=true"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}
