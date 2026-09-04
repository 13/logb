mod common;

#[tokio::test]
async fn unknown_api_route_is_json_404() {
    let app = common::spawn().await;
    let res = reqwest::get(app.url("/does-not-exist")).await.unwrap();
    assert_eq!(res.status(), 404);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "not_found");
}

#[tokio::test]
async fn spa_fallback_serves_index_or_503_when_not_built() {
    let app = common::spawn().await;
    let root = app.base.trim_end_matches("/api").to_string();
    let res = reqwest::get(format!("{root}/objects/42")).await.unwrap();
    // 200 once `frontend/dist/index.html` exists (Task 10), 503 before that.
    assert!(matches!(res.status().as_u16(), 200 | 503), "{}", res.status());
    if res.status() == 200 {
        assert!(res.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
        assert_eq!(res.headers()["cache-control"], "no-cache");
    }
}
