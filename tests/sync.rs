mod common;

#[tokio::test]
async fn every_created_row_gets_a_client_uuid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(uuid.len(), 36, "a v4 uuid in hyphenated form: {uuid}");

    let deleted: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM objects WHERE id = ?")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(deleted.is_none(), "a fresh row is not a tombstone");
}

#[tokio::test]
async fn the_log_tables_exist_and_start_empty() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let changes: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db).await.unwrap();
    let clocks: i64 = sqlx::query_scalar("SELECT count(*) FROM field_clock")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!((changes, clocks), (0, 0));
}

#[tokio::test]
async fn deleting_an_object_tombstones_it_and_its_children() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();

    // The brief wrote this as `POST /activities` with an `object_id` in the body; the real
    // route hangs activities off their object, so it is spelled the way the API is.
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities"))).json(&serde_json::json!({
        "date": "2026-01-01", "category": "fuel",
        "title": "Fill-up", "notes": "", "counter_value": 1000, "cost_cents": 5000
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "create activity: {}", res.text().await.unwrap());

    assert_eq!(
        app.client.delete(app.url(&format!("/objects/{object_id}"))).send().await.unwrap().status(),
        204
    );

    let object_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = ?")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(object_rows, 1, "the row survives; only deleted_at is set");

    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM objects WHERE id = ? AND deleted_at IS NULL")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(live, 0, "the object is tombstoned");

    let live_children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activities WHERE object_id = ? AND deleted_at IS NULL")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(live_children, 0, "children are tombstoned with the parent");

    assert_eq!(
        app.client.get(app.url(&format!("/objects/{object_id}"))).send().await.unwrap().status(),
        404,
        "a tombstoned object reads as absent"
    );
}
