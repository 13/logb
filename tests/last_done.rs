//! `GET /objects/{id}/last-done` -- see `src/api/activities.rs::last_done` and section C of
//! `docs/superpowers/specs/2026-09-15-dates-tags-last-done-design.md`.

mod common;
use serde_json::{json, Value};

async fn act(app: &common::TestApp, object_id: i64, date: &str, category: &str, title: &str, counter: Option<i64>) -> Value {
    let res = app.client
        .post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({ "date": date, "category": category, "title": title, "notes": "", "counter_value": counter }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json().await.unwrap()
}

async fn last_done(app: &common::TestApp, object_id: i64) -> reqwest::Response {
    app.client.get(app.url(&format!("/objects/{object_id}/last-done"))).send().await.unwrap()
}

#[tokio::test]
async fn groups_by_folded_title_counts_occurrences_and_reports_the_newest_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    // Same title, case- and space-varied, twice -- one folded group.
    act(&app, id, "2026-01-10", "maintenance", "Bremsbeläge vorne", Some(1000)).await;
    let may = act(&app, id, "2026-05-12", "repair", "bremsbeläge vorne ", Some(3420)).await;

    // Once each, no reminder yet -- below the occurrences >= 2 bar.
    act(&app, id, "2026-02-01", "repair", "Kette", None).await;
    act(&app, id, "2026-02-02", "repair", "Wash", None).await;

    // A `reading` category entry, even repeated, never counts -- it is not "something done".
    act(&app, id, "2026-02-03", "reading", "Stand", Some(500)).await;
    act(&app, id, "2026-02-10", "reading", "Stand", Some(600)).await;

    // Deleted twice: excluded even though the title repeats.
    let h1 = act(&app, id, "2026-02-04", "repair", "Bremsbeläge hinten", None).await;
    let h2 = act(&app, id, "2026-02-05", "repair", "Bremsbeläge hinten", None).await;
    for h in [&h1, &h2] {
        assert_eq!(app.client.delete(app.url(&format!("/activities/{}", h["id"]))).send().await.unwrap().status(), 204);
    }

    let res = last_done(&app, id).await;
    assert_eq!(res.status(), 200);
    let rows: Vec<Value> = res.json().await.unwrap();
    assert_eq!(rows.len(), 1, "only the brake pads clear the occurrences bar: {rows:?}");
    assert_eq!(rows[0]["title"], "bremsbeläge vorne", "the newest occurrence's title, trimmed");
    assert_eq!(rows[0]["occurrences"], 2);
    assert_eq!(rows[0]["last_date"], "2026-05-12");
    assert_eq!(rows[0]["last_counter"], 3420);
    assert_eq!(rows[0]["last_activity_id"], may["id"]);

    // An open reminder folds to the same title as an existing single entry and admits it too.
    let rem = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "KETTE", "due_date": "2030-01-01" }))
        .send().await.unwrap();
    assert_eq!(rem.status(), 201, "{}", rem.text().await.unwrap());

    // A DONE reminder must not have the same effect: "Wash" stays excluded.
    let wash_rem = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Wash", "due_date": "2030-01-01" }))
        .send().await.unwrap();
    assert_eq!(wash_rem.status(), 201, "{}", wash_rem.text().await.unwrap());
    let wash_rem: Value = wash_rem.json().await.unwrap();
    let done = app.client.post(app.url(&format!("/reminders/{}/done", wash_rem["id"])))
        .json(&json!({})).send().await.unwrap();
    assert_eq!(done.status(), 200, "{}", done.text().await.unwrap());

    let rows: Vec<Value> = last_done(&app, id).await.json().await.unwrap();
    let titles: Vec<&str> = rows.iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert_eq!(titles.len(), 2, "brake pads plus Kette, still without Wash: {rows:?}");
    assert!(titles.contains(&"bremsbeläge vorne"));
    let kette = rows.iter().find(|r| r["title"] == "Kette").expect("Kette from the open reminder");
    assert_eq!(kette["occurrences"], 1);
    assert!(!titles.contains(&"Wash"), "a done reminder must not pull a single occurrence in: {rows:?}");
}

/// Case folding must be full Unicode, not the ASCII-only fold SQLite's own `LOWER()` performs --
/// see the comment on `fold_title` in `src/api/activities.rs`.
#[tokio::test]
async fn folds_case_beyond_ascii() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    act(&app, id, "2026-01-01", "repair", "ÄRGER", None).await;
    let second = act(&app, id, "2026-03-01", "repair", "ärger", None).await;

    let rows: Vec<Value> = last_done(&app, id).await.json().await.unwrap();
    assert_eq!(rows.len(), 1, "ÄRGER and ärger are one folded title: {rows:?}");
    assert_eq!(rows[0]["title"], "ärger");
    assert_eq!(rows[0]["occurrences"], 2);
    assert_eq!(rows[0]["last_activity_id"], second["id"]);
}

#[tokio::test]
async fn orders_newest_date_first_and_breaks_a_tie_by_the_higher_activity_id() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    act(&app, id, "2026-01-01", "repair", "Oldest", None).await;
    act(&app, id, "2026-01-05", "repair", "Oldest", None).await;

    act(&app, id, "2026-02-01", "repair", "Newest", None).await;
    act(&app, id, "2026-02-20", "repair", "Newest", None).await;

    // Two different titles whose LAST occurrence lands on the same date: the one inserted
    // (and so given the higher id) second must sort first.
    act(&app, id, "2026-02-10", "repair", "Tie A", None).await;
    act(&app, id, "2026-02-10", "repair", "Tie A", None).await;
    act(&app, id, "2026-02-10", "repair", "Tie B", None).await;
    let tie_b_last = act(&app, id, "2026-02-10", "repair", "Tie B", None).await;

    let rows: Vec<Value> = last_done(&app, id).await.json().await.unwrap();
    let titles: Vec<&str> = rows.iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Newest", "Tie B", "Tie A", "Oldest"], "{rows:?}");
    assert_eq!(rows[1]["last_activity_id"], tie_b_last["id"]);
}

#[tokio::test]
async fn answers_404_for_another_users_object_and_an_empty_list_with_no_entries() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let mine = app.create_object(&app.client, "Golf", Some("km")).await;
    let mine_id = mine["id"].as_i64().unwrap();
    act(&app, mine_id, "2026-01-01", "repair", "Öl", None).await;
    act(&app, mine_id, "2026-01-02", "repair", "Öl", None).await;

    let res = anna.get(app.url(&format!("/objects/{mine_id}/last-done"))).send().await.unwrap();
    assert_eq!(res.status(), 404);

    let empty = app.create_object(&app.client, "Bare", Some("km")).await;
    let rows: Vec<Value> = last_done(&app, empty["id"].as_i64().unwrap()).await.json().await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn caps_at_fifty_rows() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    for i in 0..55 {
        let title = format!("Title {i:02}");
        act(&app, id, &format!("2026-01-{:02}", (i % 28) + 1), "repair", &title, None).await;
        act(&app, id, &format!("2026-06-{:02}", (i % 28) + 1), "repair", &title, None).await;
    }
    let rows: Vec<Value> = last_done(&app, id).await.json().await.unwrap();
    assert_eq!(rows.len(), 50);
}
