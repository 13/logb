mod common;
use serde_json::{json, Value};

async fn post(app: &common::TestApp, client: &reqwest::Client, path: &str, body: Value) -> reqwest::Response {
    client.post(app.url(path)).json(&body).send().await.unwrap()
}

#[tokio::test]
async fn objects_and_entries_carry_normalised_tags() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = post(&app, &app.client, "/objects", json!({
        "name": "Golf", "type": "car", "description": "", "tags": [" Lease ", "winter", "Winter"]
    })).await;
    assert_eq!(res.status(), 201);
    let car: Value = res.json().await.unwrap();
    assert_eq!(car["tags"], json!(["Lease", "winter"]));
    let id = car["id"].as_i64().unwrap();

    // Absent on update keeps the tags.
    let res = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({ "name": "Golf VII", "type": "car", "description": "" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.json::<Value>().await.unwrap()["tags"], json!(["Lease", "winter"]));

    let res = post(&app, &app.client, &format!("/objects/{id}/activities"), json!({
        "date": "2026-03-01", "category": "repair", "title": "Tyres", "notes": "", "tags": ["Winter", "tax 2026"]
    })).await;
    assert_eq!(res.status(), 201);
    assert_eq!(res.json::<Value>().await.unwrap()["tags"], json!(["Winter", "tax 2026"]));

    let listed = app.get_json("/objects?all=true").await;
    assert_eq!(listed[0]["tags"], json!(["Lease", "winter"]));
}

#[tokio::test]
async fn limits_answer_400() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let many: Vec<String> = (0..11).map(|i| format!("t{i}")).collect();
    let res = post(&app, &app.client, "/objects", json!({ "name": "X", "type": "car", "description": "", "tags": many })).await;
    assert_eq!(res.status(), 400);
    let res = post(&app, &app.client, "/objects", json!({ "name": "X", "type": "car", "description": "", "tags": ["x".repeat(33)] })).await;
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn tag_list_counts_per_user_with_the_most_used_spelling() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    for (name, tags) in [("A", json!(["Winter"])), ("B", json!(["winter", "Lease"])), ("C", json!(["Winter"]))] {
        assert_eq!(post(&app, &app.client, "/objects", json!({ "name": name, "type": "car", "description": "", "tags": tags })).await.status(), 201);
    }
    assert_eq!(post(&app, &anna, "/objects", json!({ "name": "Z", "type": "car", "description": "", "tags": ["Private"] })).await.status(), 201);
    assert_eq!(app.get_json("/tags").await, json!([{ "tag": "Winter", "count": 3 }, { "tag": "Lease", "count": 1 }]));
}

#[tokio::test]
async fn entries_filter_by_tag_ignoring_case_and_accents() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    for (title, tags) in [("One", json!(["Fahrräder"])), ("Two", json!(["other"])), ("Three", json!(["FAHRRADER"]))] {
        let res = post(&app, &app.client, &format!("/objects/{id}/activities"), json!({ "date": "2026-03-01", "category": "repair", "title": title, "notes": "", "tags": tags })).await;
        assert_eq!(res.status(), 201);
    }
    let res = app.client.get(app.url(&format!("/objects/{id}/activities?tag=fahrrader"))).send().await.unwrap();
    assert_eq!(res.headers()["x-total-count"], "2");
    let rows: Value = res.json().await.unwrap();
    let mut titles: Vec<&str> = rows.as_array().unwrap().iter().map(|a| a["title"].as_str().unwrap()).collect();
    titles.sort();
    assert_eq!(titles, ["One", "Three"]);
}

#[tokio::test]
async fn search_finds_tags() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert_eq!(post(&app, &app.client, "/objects", json!({ "name": "Golf", "type": "car", "description": "", "tags": ["Leasing"] })).await.status(), 201);
    let out = app.get_json("/search?q=leasing").await;
    assert_eq!(out["objects"][0]["name"], "Golf");
}

/// Tags are stored as JSON text, so a LIKE over the column would match its punctuation:
/// `[` would find every row and `"` every tagged one. Such terms skip the tags match.
#[tokio::test]
async fn search_for_json_punctuation_does_not_match_the_tags_column() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert_eq!(post(&app, &app.client, "/objects", json!({ "name": "Golf", "type": "car", "description": "", "tags": ["Lease"] })).await.status(), 201);
    assert_eq!(post(&app, &app.client, "/objects", json!({ "name": "Bike", "type": "bike", "description": "" })).await.status(), 201);
    let car = app.create_object(&app.client, "Van", Some("km")).await;
    let res = post(&app, &app.client, &format!("/objects/{}/activities", car["id"]), json!({
        "date": "2026-03-01", "category": "repair", "title": "Tyres", "notes": "", "tags": ["Lease"]
    })).await;
    assert_eq!(res.status(), 201);
    // `%22Lease` is `"Lease`, which the raw JSON text `["Lease"]` contains.
    for q in ["%5B", "%5D", "%22", "%2C", "%5C", "%22Lease"] {
        let out = app.get_json(&format!("/search?q={q}")).await;
        assert_eq!(out["objects"], json!([]), "q={q}: {out}");
    }
    for q in ["%5B", "%22Lease"] {
        let out = app.get_json(&format!("/search?q={q}")).await;
        assert_eq!(out["activities"], json!([]), "q={q}: {out}");
    }
}

async fn titles_and_total(app: &common::TestApp, path: &str) -> (Vec<String>, String) {
    let res = app.client.get(app.url(path)).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let total = res.headers()["x-total-count"].to_str().unwrap().to_string();
    let rows: Value = res.json().await.unwrap();
    (rows.as_array().unwrap().iter().map(|a| a["title"].as_str().unwrap().to_string()).collect(), total)
}

/// The tag filter runs after the query, so paging and the total must count only tagged rows.
#[tokio::test]
async fn the_tag_filter_pages_and_counts_only_tagged_entries() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    // Tagged T1..T5 on days 1..5 interleaved with untagged U1..U5 on days 11..15; categories mixed.
    for i in 1..=5 {
        let category = if i % 2 == 0 { "fuel" } else { "repair" };
        let res = post(&app, &app.client, &format!("/objects/{id}/activities"), json!({
            "date": format!("2026-03-{:02}", i * 2 - 1), "category": category, "title": format!("T{i}"), "notes": "", "tags": ["Winter"]
        })).await;
        assert_eq!(res.status(), 201);
        let res = post(&app, &app.client, &format!("/objects/{id}/activities"), json!({
            "date": format!("2026-03-{:02}", i * 2), "category": category, "title": format!("U{i}"), "notes": ""
        })).await;
        assert_eq!(res.status(), 201);
    }

    let (titles, total) = titles_and_total(&app, &format!("/objects/{id}/activities?tag=winter&limit=2&offset=2")).await;
    assert_eq!(titles, ["T3", "T2"], "date DESC: T5, T4 | T3, T2 | T1");
    assert_eq!(total, "5");

    let (titles, total) = titles_and_total(&app, &format!("/objects/{id}/activities?tag=winter&category=repair")).await;
    assert_eq!(titles, ["T5", "T3", "T1"]);
    assert_eq!(total, "3");
}
