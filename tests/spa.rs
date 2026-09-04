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

/// `/api` and `/api/` (no trailing segment) must still get the JSON error body, not the
/// HTML app shell.
#[tokio::test]
async fn bare_api_path_is_json_404() {
    let app = common::spawn().await;
    let root = app.base.trim_end_matches("/api").to_string();

    for path in ["/api", "/api/"] {
        let res = reqwest::get(format!("{root}{path}")).await.unwrap();
        assert_eq!(res.status(), 404, "path {path}");
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["error"], "not_found", "path {path}");
    }
}

/// A non-canonical path under `/api` (double slash) must still get the JSON error body
/// rather than falling through to the HTML app shell.
///
/// Note: `/./api/does-not-exist` is NOT exercised here. `reqwest` (via the `url` crate)
/// removes `.` dot-segments while parsing the request URL, so by the time the request
/// reaches the server the path is already the canonical `/api/does-not-exist` - asserting
/// on it here would pass vacuously without exercising the fix. That case is covered
/// instead by a unit test of `spa::normalize` in `src/spa.rs`
/// (`dot_segment_prefix_is_still_api`), which operates on the raw path string. The `url`
/// crate does not collapse a leading `//`, so that bypass reaches the server unmodified and
/// is meaningfully covered here.
#[tokio::test]
async fn double_slash_api_path_is_json_404() {
    let app = common::spawn().await;
    let root = app.base.trim_end_matches("/api").to_string();
    let res = reqwest::get(format!("{root}//api/does-not-exist")).await.unwrap();
    assert_eq!(res.status(), 404);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "not_found");
}
