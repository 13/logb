//! `/metrics`: off unless configured, and then only for the configured bearer token.

use crate::common::{spawn, spawn_with, TestApp};

const TOKEN: &str = "scrape-me-9c1f";

/// `/metrics` sits at the root, beside `/api`, where a scraper looks by default.
fn metrics_url(app: &TestApp) -> String {
    format!("{}/metrics", app.base.strip_suffix("/api").expect("the harness serves under /api"))
}

async fn scrape(app: &TestApp, token: Option<&str>) -> reqwest::Response {
    let mut req = reqwest::Client::new().get(metrics_url(app));
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    req.send().await.unwrap()
}

async fn enabled() -> TestApp {
    spawn_with(|c| c.metrics_token = Some(TOKEN.into())).await
}

/// Unset, the route is not there -- and in particular the SPA fallback does not answer it with
/// `index.html` and a 200, which a scraper would try to parse and a person would read as "on".
#[tokio::test]
async fn unset_the_endpoint_answers_404_whatever_is_sent() {
    let app = spawn().await;
    for token in [None, Some(TOKEN), Some("")] {
        let res = scrape(&app, token).await;
        assert_eq!(res.status(), 404, "token {token:?}");
        let body = res.text().await.unwrap();
        assert!(!body.contains("<html"), "the SPA answered /metrics: {body}");
    }
}

/// A blank token is not a token: an exported-but-empty variable must not open the endpoint to
/// an empty bearer.
#[tokio::test]
async fn a_blank_token_counts_as_unset() {
    let app = spawn_with(|c| c.metrics_token = Some("  ".into())).await;
    assert_eq!(scrape(&app, Some("  ")).await.status(), 404);
    assert_eq!(scrape(&app, None).await.status(), 404);
}

#[tokio::test]
async fn set_it_refuses_a_missing_or_wrong_token() {
    let app = enabled().await;
    for token in [None, Some("wrong"), Some("scrape-me-9c1"), Some("scrape-me-9c1ff")] {
        let res = scrape(&app, token).await;
        assert_eq!(res.status(), 401, "token {token:?}");
        assert_eq!(res.headers()["www-authenticate"], "Bearer");
    }
    // A session is not a way in either: this is for a scraper, not for a signed-in person.
    app.setup("admin", "correct-horse-battery").await;
    let res = app.client.get(metrics_url(&app)).send().await.unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn the_right_token_gets_prometheus_text_with_every_metric() {
    let app = enabled().await;
    let res = scrape(&app, Some(TOKEN)).await;
    assert_eq!(res.status(), 200);
    assert!(
        res.headers()["content-type"].to_str().unwrap().starts_with("text/plain; version=0.0.4"),
        "{:?}",
        res.headers()["content-type"]
    );
    let body = res.text().await.unwrap();
    for name in [
        "logb_build_info",
        "process_start_time_seconds",
        "logb_http_requests_total",
        "logb_http_request_duration_seconds",
        "logb_db_pool_connections",
        "logb_db_pool_idle_connections",
        "logb_db_pool_max_connections",
        "logb_db_write_lock_wait_seconds",
        "logb_db_write_lock_timeouts_total",
    ] {
        assert!(body.contains(&format!("# TYPE {name} ")), "{name} missing:\n{body}");
    }
    assert!(
        body.contains(&format!("logb_build_info{{version=\"{}\"}} 1\n", env!("CARGO_PKG_VERSION"))),
        "{body}"
    );
    assert!(body.contains("logb_db_pool_max_connections{pool=\"write\"}"), "{body}");
}

fn counter(body: &str, labels: &str) -> u64 {
    let prefix = format!("logb_http_requests_total{{{labels}}} ");
    body.lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .map_or(0, |n| n.parse().unwrap())
}

/// A request is counted under its route *template*, never its raw path, so ids do not mint
/// series; a write shows up in the write-lock histogram.
#[tokio::test]
async fn requests_are_counted_by_route_template_and_status_class() {
    let app = enabled().await;
    app.setup("admin", "correct-horse-battery").await;
    let object = app.create_object(&app.client, "Van", Some("km")).await;
    let id = object["id"].as_i64().unwrap();

    let before = scrape(&app, Some(TOKEN)).await.text().await.unwrap();
    let ok = r#"route="/api/objects/{id}",method="GET",status="2xx""#;
    let missing = r#"route="/api/objects/{id}",method="GET",status="4xx""#;
    app.get_json(&format!("/objects/{id}")).await;
    app.get_json(&format!("/objects/{id}")).await;
    let res = app.client.get(app.url("/objects/999999")).send().await.unwrap();
    assert_eq!(res.status(), 404);
    let after = scrape(&app, Some(TOKEN)).await.text().await.unwrap();

    assert_eq!(counter(&after, ok) - counter(&before, ok), 2, "{after}");
    assert_eq!(counter(&after, missing) - counter(&before, missing), 1, "{after}");
    assert!(!after.contains(&format!("/api/objects/{id}\"")), "a raw path became a label:\n{after}");
    // The scrape itself is counted under its own route, and a path no route matched under one
    // shared label rather than one per path.
    assert!(after.contains(r#"route="/metrics",method="GET",status="2xx""#), "{after}");
    let _ = app.client.get(app.url("/no-such-thing-1")).send().await.unwrap();
    let _ = app.client.get(app.url("/no-such-thing-2")).send().await.unwrap();
    let last = scrape(&app, Some(TOKEN)).await.text().await.unwrap();
    assert!(!last.contains("no-such-thing"), "{last}");
    assert!(counter(&last, r#"route="fallback",method="GET",status="4xx""#) >= 2, "{last}");

    // Setup and the object create both wrote.
    let writes: u64 = after
        .lines()
        .find_map(|l| l.strip_prefix("logb_db_write_lock_wait_seconds_count "))
        .unwrap()
        .parse()
        .unwrap();
    assert!(writes >= 2, "{after}");
}
