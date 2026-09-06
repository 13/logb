mod common;
use serde_json::json;

fn act(date: &str, category: &str, counter: Option<i64>, cost: Option<i64>) -> serde_json::Value {
    json!({ "date": date, "category": category, "title": format!("{category} on {date}"),
            "notes": "", "counter_value": counter, "cost_cents": cost })
}

#[tokio::test]
async fn timeline_totals_and_counter() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let base = app.url(&format!("/objects/{id}/activities"));

    for a in [
        act("2024-01-10", "maintenance", Some(100_000), Some(25_000)),
        act("2024-06-01", "repair", Some(104_500), Some(80_000)),
        act("2024-03-15", "fuel", Some(102_000), None),
    ] {
        let res = app.client.post(&base).json(&a).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let list: Vec<serde_json::Value> = app.client.get(&base).send().await.unwrap().json().await.unwrap();
    let dates: Vec<&str> = list.iter().map(|a| a["date"].as_str().unwrap()).collect();
    assert_eq!(dates, ["2024-06-01", "2024-03-15", "2024-01-10"], "newest first");

    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(obj["stats"]["total_cost_cents"], 105_000);
    assert_eq!(obj["stats"]["activity_count"], 3);
    assert_eq!(obj["stats"]["current_counter"], 104_500);

    let fuel: Vec<serde_json::Value> = app.client.get(format!("{base}?category=fuel")).send().await.unwrap().json().await.unwrap();
    assert_eq!(fuel.len(), 1);
    let h1: Vec<serde_json::Value> = app.client.get(format!("{base}?from=2024-01-01&to=2024-03-31")).send().await.unwrap().json().await.unwrap();
    assert_eq!(h1.len(), 2);

    let aid = list[0]["id"].as_i64().unwrap();
    let res = app.client.patch(app.url(&format!("/activities/{aid}"))).json(&act("2024-06-02", "repair", Some(104_600), Some(90_000))).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let one: serde_json::Value = app.client.get(app.url(&format!("/activities/{aid}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(one["cost_cents"], 90_000);

    assert_eq!(app.client.delete(app.url(&format!("/activities/{aid}"))).send().await.unwrap().status(), 204);
    let obj: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(obj["stats"]["total_cost_cents"], 25_000);
    assert_eq!(obj["stats"]["current_counter"], 102_000);
}

#[tokio::test]
async fn validation() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let home = app.create_object(&app.client, "Home", None).await;
    let car_base = app.url(&format!("/objects/{}/activities", car["id"]));
    let home_base = app.url(&format!("/objects/{}/activities", home["id"]));
    for (base, body) in [
        (&car_base, act("2024-13-01", "repair", None, None)),
        (&car_base, act("2024-01-01", "party", None, None)),
        (&car_base, json!({ "date": "2024-01-01", "category": "repair", "title": "  " })),
        (&car_base, act("2024-01-01", "repair", Some(-5), None)),
        (&car_base, act("2024-01-01", "repair", None, Some(-1))),
        (&home_base, act("2024-01-01", "repair", Some(10), None)),
    ] {
        let res = app.client.post(base).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 400, "{body}");
    }
}

#[tokio::test]
async fn isolation() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let base = app.url(&format!("/objects/{}/activities", car["id"]));
    let a: serde_json::Value = app.client.post(&base).json(&act("2024-01-01", "repair", None, Some(1))).send().await.unwrap().json().await.unwrap();
    assert_eq!(anna.get(&base).send().await.unwrap().status(), 404);
    assert_eq!(anna.post(&base).json(&act("2024-01-01", "repair", None, None)).send().await.unwrap().status(), 404);
    assert_eq!(anna.get(app.url(&format!("/activities/{}", a["id"]))).send().await.unwrap().status(), 404);
    assert_eq!(anna.patch(app.url(&format!("/activities/{}", a["id"]))).json(&act("2024-01-01", "repair", None, None)).send().await.unwrap().status(), 404);
    assert_eq!(anna.delete(app.url(&format!("/activities/{}", a["id"]))).send().await.unwrap().status(), 404);
}

/// A long timeline comes back a page at a time, with the unpaged total in a header so the
/// client knows whether to offer "show older".
#[tokio::test]
async fn the_activity_list_is_paged() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", None).await;
    let id = car["id"].as_i64().unwrap();
    for i in 0..7 {
        app.client.post(app.url(&format!("/objects/{id}/activities")))
            .json(&json!({ "date": format!("2024-01-{:02}", i + 1), "category": "fuel", "title": format!("Fill {i}") }))
            .send().await.unwrap();
    }

    let res = app.client.get(app.url(&format!("/objects/{id}/activities?limit=3"))).send().await.unwrap();
    assert_eq!(res.headers()["x-total-count"], "7", "the header counts everything, not the page");
    let page1: serde_json::Value = res.json().await.unwrap();
    assert_eq!(page1.as_array().unwrap().len(), 3);
    assert_eq!(page1[0]["title"], "Fill 6", "newest first");

    let page3: serde_json::Value = app.client.get(app.url(&format!("/objects/{id}/activities?limit=3&offset=6")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(page3.as_array().unwrap().len(), 1);
    assert_eq!(page3[0]["title"], "Fill 0", "the last page holds the oldest entry");

    // The total tracks the filter, not the table.
    let res = app.client.get(app.url(&format!("/objects/{id}/activities?category=repair"))).send().await.unwrap();
    assert_eq!(res.headers()["x-total-count"], "0");

    // Absurd limits are clamped rather than rejected.
    let res = app.client.get(app.url(&format!("/objects/{id}/activities?limit=99999"))).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let all: serde_json::Value = res.json().await.unwrap();
    assert_eq!(all.as_array().unwrap().len(), 7);
}

#[tokio::test]
async fn recent_titles_are_distinct_and_newest_first() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    for (date, title, cost, counter) in [
        ("2026-01-05", "Fuel", 5000, 10_000),
        ("2026-02-05", "Oil change", 9000, 11_000),
        ("2026-03-05", "Fuel", 6210, 12_000),
    ] {
        let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
            "date": date, "category": "fuel", "title": title,
            "cost_cents": cost, "counter_value": counter
        })).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let out: Vec<serde_json::Value> = app.client
        .get(app.url(&format!("/objects/{id}/recent-titles")))
        .send().await.unwrap().json().await.unwrap();

    assert_eq!(out.len(), 2, "one row per distinct (title, category)");
    assert_eq!(out[0]["title"], "Fuel", "the most recent title comes first");
    assert_eq!(out[0]["last_date"], "2026-03-05");
    assert_eq!(out[0]["last_cost_cents"], 6210, "the newest occurrence supplies the cost");
    assert_eq!(out[0]["last_counter"], 12_000);
    assert_eq!(out[1]["title"], "Oil change");
}

#[tokio::test]
async fn recent_titles_of_another_users_object_are_404() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = anna.get(app.url(&format!("/objects/{id}/recent-titles"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}
