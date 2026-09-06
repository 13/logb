mod common;
use serde_json::json;

async fn seed(app: &common::TestApp) -> (i64, i64) {
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let bike = app.create_object(&app.client, "Cube Bike", None).await;
    let (car_id, bike_id) = (car["id"].as_i64().unwrap(), bike["id"].as_i64().unwrap());
    app.client.post(app.url(&format!("/objects/{car_id}/activities")))
        .json(&json!({ "date": "2024-03-01", "category": "maintenance", "title": "Oil change", "notes": "Castrol 5W-30" }))
        .send().await.unwrap();
    app.client.post(app.url(&format!("/objects/{bike_id}/activities")))
        .json(&json!({ "date": "2024-04-01", "category": "repair", "title": "New chain", "notes": "worn out" }))
        .send().await.unwrap();
    (car_id, bike_id)
}

#[tokio::test]
async fn finds_objects_and_activities() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (car_id, _) = seed(&app).await;

    // Matches an activity title, not an object.
    let r: serde_json::Value = app.client.get(app.url("/search?q=oil")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0, "{r}");
    let acts = r["activities"].as_array().unwrap();
    assert_eq!(acts.len(), 1, "{r}");
    assert_eq!(acts[0]["title"], "Oil change");
    assert_eq!(acts[0]["object_id"], car_id);
    assert_eq!(acts[0]["object_name"], "Golf", "an activity hit carries its object's name");

    // Matches an object name, case-insensitively.
    let r: serde_json::Value = app.client.get(app.url("/search?q=GOLF")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["objects"][0]["name"], "Golf");

    // Matches activity notes.
    let r: serde_json::Value = app.client.get(app.url("/search?q=castrol")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["activities"].as_array().unwrap().len(), 1, "{r}");

    // No hits is an empty result, not an error.
    let r: serde_json::Value = app.client.get(app.url("/search?q=zzzz")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0);
    assert_eq!(r["activities"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn wildcards_in_the_query_are_literal() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;
    // `%` would match everything if it reached LIKE unescaped.
    let r: serde_json::Value = app.client.get(app.url("/search?q=%25")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0, "{r}");
    assert_eq!(r["activities"].as_array().unwrap().len(), 0, "{r}");
}

#[tokio::test]
async fn search_is_scoped_to_the_caller() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;
    let eve = app.create_user_client("eve", "password123").await;
    let r: serde_json::Value = eve.get(app.url("/search?q=golf")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0, "another user's objects must not surface: {r}");
    assert_eq!(r["activities"].as_array().unwrap().len(), 0, "{r}");
}

#[tokio::test]
async fn blank_and_anonymous_queries_are_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert_eq!(app.client.get(app.url("/search?q=%20")).send().await.unwrap().status(), 400);
    assert_eq!(app.client.get(app.url("/search")).send().await.unwrap().status(), 400);
    assert_eq!(common::new_client().get(app.url("/search?q=golf")).send().await.unwrap().status(), 401);
}
