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

/// `type` holds an identifier -- `car`, `e_bike`, `other` -- not words anyone typed, and search
/// used to match it. That made a search box that answered questions about the schema: a German
/// user searching "Auto" found nothing while "car" found their Golf, "other" returned every
/// unclassified object at once, and "bike" dragged in every e-bike alongside the bicycles.
///
/// Unmapped legacy text was preserved into the description by the migration, so an object whose
/// old free-text category meant something to its owner is still found by those words.
#[tokio::test]
async fn objects_are_found_by_their_words_not_their_type() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let golf = app.create_object(&app.client, "Golf", Some("km")).await;
    assert_eq!(golf["type"], "car", "the fixture files this one as a car");

    let r: serde_json::Value = app.client.get(app.url("/search?q=car")).send().await.unwrap().json().await.unwrap();
    assert_eq!(
        r["objects"].as_array().unwrap().len(), 0,
        "`car` is a stored identifier, not something the user typed: {r}",
    );

    let r: serde_json::Value = app.client.get(app.url("/search?q=Golf")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "the name the user chose still finds it: {r}");
    assert_eq!(r["objects"][0]["id"], golf["id"]);

    // What the migration preserved is what a legacy owner will search for: an object whose old
    // free-text category did not map carries those words in its description.
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Odd one", "type": "other", "counter_unit": null,
                       "description": "Gravelbike Custom", "purchase_date": null,
                       "purchase_price_cents": null }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let r: serde_json::Value = app.client.get(app.url("/search?q=gravelbike")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "text kept in the description stays findable: {r}");

    // And the type of the object that carries it is still not a search term.
    let r: serde_json::Value = app.client.get(app.url("/search?q=other")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0, "`other` must not return every unclassified object: {r}");
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

#[tokio::test]
async fn search_paginates_without_duplicates() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Pagination object", None).await;
    for n in 0..30 {
        app.create_activity(&object["id"], &format!("pagination-{n}")).await;
    }
    let first: serde_json::Value = app.client.get(app.url("/search?q=pagination&limit=10&offset=0")).send().await.unwrap().json().await.unwrap();
    let second: serde_json::Value = app.client.get(app.url("/search?q=pagination&limit=10&offset=10")).send().await.unwrap().json().await.unwrap();
    assert_eq!(first["activities"].as_array().unwrap().len(), 10);
    assert_eq!(second["activities"].as_array().unwrap().len(), 10);
    assert!(first["has_more"].as_bool().unwrap());
    let ids: std::collections::HashSet<_> = first["activities"].as_array().unwrap().iter().chain(second["activities"].as_array().unwrap().iter()).map(|a| a["id"].as_i64().unwrap()).collect();
    assert_eq!(ids.len(), 20);
}

/// An object hit says which object it sits inside, so a list of four things called "Filter"
/// can be told apart without opening any of them.
#[tokio::test]
async fn a_search_hit_names_its_parent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light unique term", None).await;
    app.client.patch(app.url(&format!("/objects/{}", light["id"])))
        .json(&json!({ "name": "Main light unique term", "type": "other",
            "counter_unit": null, "description": "", "purchase_date": null,
            "purchase_price_cents": null, "parent_id": garage["id"] }))
        .send().await.unwrap();

    let res: serde_json::Value = app.client.get(app.url("/search?q=unique+term"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(res["objects"][0]["parent_name"], "Garage");
}

/// A root object has no parent to name, and says so with a null rather than an empty string:
/// the UI draws the line only when there is a parent.
#[tokio::test]
async fn a_root_objects_search_hit_has_no_parent_name() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Solo object distinctive", None).await;
    let res: serde_json::Value = app.client.get(app.url("/search?q=distinctive"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(res["objects"][0]["parent_name"], serde_json::Value::Null);
}

/// A trip's places are searched like its title and notes -- and the umlaut proves this runs
/// through the backend's own case-folding (`state.backend.case_insensitive_like()`), not an
/// ASCII-only shortcut.
#[tokio::test]
async fn a_trip_is_found_by_its_places() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let id = bike["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-06-01", "category": "trip", "title": "", "notes": "",
        "start_counter": 400, "counter_value": 600,
        "from_place": "Bäckerei Müller", "to_place": "Home"
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    let r: serde_json::Value = app.client.get(app.url("/search?q=B%C3%A4ckerei")).send().await.unwrap().json().await.unwrap();
    let acts = r["activities"].as_array().unwrap();
    assert_eq!(acts.len(), 1, "{r}");
    assert_eq!(acts[0]["category"], "trip");
}
