mod common;
use serde_json::json;

#[tokio::test]
async fn crud_and_stats() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    assert_eq!(car["name"], "Golf");
    assert_eq!(car["counter_unit"], "km");
    assert_eq!(car["stats"]["total_cost_cents"], 0);
    assert_eq!(car["stats"]["activity_count"], 0);
    assert!(car["stats"]["current_counter"].is_null());
    let id = car["id"].as_i64().unwrap();

    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1);

    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({
        "name": "Golf VII", "category": "car", "counter_unit": "km", "description": "grey",
        "purchase_date": "2020-03-01", "purchase_price_cents": 1500000, "archived": true
    })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let car: serde_json::Value = res.json().await.unwrap();
    assert_eq!(car["name"], "Golf VII");
    assert!(car["archived_at"].is_string());

    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 0, "archived hidden by default");
    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects?archived=true")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1);

    assert_eq!(app.client.delete(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 204);
    assert_eq!(app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 404);
}

#[tokio::test]
async fn validation() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for body in [
        json!({ "name": " ", "category": "car" }),
        json!({ "name": "x", "category": "" }),
        json!({ "name": "x", "category": "car", "counter_unit": "furlongs" }),
        json!({ "name": "x", "category": "car", "purchase_date": "01.03.2020" }),
        json!({ "name": "x", "category": "car", "purchase_price_cents": -1 }),
    ] {
        let res = app.client.post(app.url("/objects")).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 400, "{body}");
    }
}

#[tokio::test]
async fn other_users_objects_are_invisible() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let list: Vec<serde_json::Value> = anna.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert!(list.is_empty());
    assert_eq!(anna.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 404);
    assert_eq!(anna.patch(app.url(&format!("/objects/{id}"))).json(&json!({ "name": "pwned", "category": "car" })).send().await.unwrap().status(), 404);
    assert_eq!(anna.delete(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 404);
    assert_eq!(app.client.get(app.url(&format!("/objects/{id}"))).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn requires_login() {
    let app = common::spawn().await;
    assert_eq!(reqwest::get(app.url("/objects")).await.unwrap().status(), 401);
}

#[tokio::test]
async fn fuel_unit_round_trips_and_is_validated() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "E-bike", "category": "bike", "counter_unit": "km", "fuel_unit": "kwh"
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let bike: serde_json::Value = res.json().await.unwrap();
    assert_eq!(bike["fuel_unit"], "kwh");

    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Car", "category": "car", "fuel_unit": "barrels"
    })).send().await.unwrap();
    assert_eq!(res.status(), 400);
}
