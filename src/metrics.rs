//! `/metrics`: a Prometheus scrape target, off unless `LOGB_METRICS_TOKEN` is set.
//!
//! Hand-written rather than a metrics crate: a handful of counters and two histograms need
//! nothing more than atomics and the text exposition format, and none of the crates would be
//! smaller than this file.
//!
//! The route lives at the root, beside `/api` rather than inside it, because `/metrics` is where
//! every scraper looks by default. It is always routed, enabled or not: left out, the SPA
//! fallback would answer `/metrics` with `index.html` and a 200, which a scraper would try to
//! parse and a person would read as "metrics are on". Disabled, it answers the same 404 an
//! unknown `/api` path does.
//!
//! What it costs a request: one read-locked map lookup and three atomic adds. The lock is only
//! ever taken for writing the first time a route/method/status-class combination is seen, and
//! those are bounded by the router -- the route label is axum's matched *template*
//! (`/api/objects/{id}`), never the raw path, so no client can mint new series.

use crate::error::AppError;
use crate::state::App;
use axum::extract::{MatchedPath, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Upper bounds, in seconds, shared by every histogram here. From a millisecond (a cached read)
/// to ten seconds (past `db::WRITE_WAIT`, so a write that timed out still lands in a bucket).
const BUCKETS: [f64; 12] = [0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

/// The route label for a request no route matched: the embedded frontend (every asset and
/// `index.html`) and unknown `/api` paths alike -- one series, however many paths there are.
const FALLBACK: &str = "fallback";

#[derive(Default)]
struct Histogram {
    /// Per-bucket counts, not cumulative; `render` sums them. An observation above the last
    /// bound is only in `count`, which is the `+Inf` bucket.
    buckets: [AtomicU64; BUCKETS.len()],
    count: AtomicU64,
    sum_nanos: AtomicU64,
}

impl Histogram {
    fn observe(&self, elapsed: Duration) {
        let secs = elapsed.as_secs_f64();
        if let Some(i) = BUCKETS.iter().position(|le| secs <= *le) {
            self.buckets[i].fetch_add(1, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_nanos
            .fetch_add(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    /// The `_bucket`, `_sum` and `_count` lines, `labels` being the series' own labels already
    /// joined (`route="..",method=".."`), or empty.
    fn render(&self, out: &mut String, name: &str, labels: &str) {
        let sep = if labels.is_empty() { "" } else { "," };
        let mut cumulative = 0;
        for (le, n) in BUCKETS.iter().zip(&self.buckets) {
            cumulative += n.load(Ordering::Relaxed);
            let _ = writeln!(out, "{name}_bucket{{{labels}{sep}le=\"{le}\"}} {cumulative}");
        }
        let count = self.count.load(Ordering::Relaxed);
        let _ = writeln!(out, "{name}_bucket{{{labels}{sep}le=\"+Inf\"}} {count}");
        let sum = self.sum_nanos.load(Ordering::Relaxed) as f64 / 1e9;
        let braces = |s: &str| if s.is_empty() { String::new() } else { format!("{{{s}}}") };
        let _ = writeln!(out, "{name}_sum{} {sum}", braces(labels));
        let _ = writeln!(out, "{name}_count{} {count}", braces(labels));
    }
}

/// The methods a series may be labelled with. Anything else a client sends is `other`, so an
/// invented method cannot add a series either.
fn method_label(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::HEAD => "HEAD",
        Method::OPTIONS => "OPTIONS",
        _ => "other",
    }
}

/// `1xx` .. `5xx`; a status outside those ranges cannot be constructed by `http`.
fn status_class(status: StatusCode) -> u8 {
    (status.as_u16() / 100) as u8
}

type RouteSeries = HashMap<(&'static str, u8), Histogram>;

/// Everything `/metrics` reports that is not read live off the pools at scrape time.
pub struct Metrics {
    started: SystemTime,
    /// Route template -> (method, status class) -> latency. Nested so a lookup by `&str` needs
    /// no allocation.
    http: RwLock<HashMap<Box<str>, RouteSeries>>,
    write_wait: Histogram,
    write_timeouts: AtomicU64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Metrics {
            started: SystemTime::now(),
            http: RwLock::new(HashMap::new()),
            write_wait: Histogram::default(),
            write_timeouts: AtomicU64::default(),
        }
    }

    fn observe_request(&self, route: &str, method: &'static str, class: u8, elapsed: Duration) {
        {
            let map = self.http.read().unwrap_or_else(|e| e.into_inner());
            if let Some(h) = map.get(route).and_then(|r| r.get(&(method, class))) {
                h.observe(elapsed);
                return;
            }
        }
        let mut map = self.http.write().unwrap_or_else(|e| e.into_inner());
        map.entry(route.into())
            .or_default()
            .entry((method, class))
            .or_default()
            .observe(elapsed);
    }

    /// How long `db::begin_write` waited before it held the write lock -- the writer connection
    /// on SQLite, the pool plus the advisory lock on PostgreSQL -- and whether it gave up.
    pub fn observe_write_wait(&self, elapsed: Duration, timed_out: bool) {
        self.write_wait.observe(elapsed);
        if timed_out {
            self.write_timeouts.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn render(&self, state: &App) -> String {
        let mut out = String::with_capacity(8 * 1024);

        out.push_str("# HELP logb_build_info The running LogB version; the value is always 1.\n");
        out.push_str("# TYPE logb_build_info gauge\n");
        let _ = writeln!(out, "logb_build_info{{version=\"{}\"}} 1", escape(env!("CARGO_PKG_VERSION")));

        out.push_str("# HELP process_start_time_seconds Start time of the process since the Unix epoch, in seconds.\n");
        out.push_str("# TYPE process_start_time_seconds gauge\n");
        let started = self.started.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
        let _ = writeln!(out, "process_start_time_seconds {started}");

        // Sorted, so two scrapes list series in the same order and a diff of them reads.
        let map = self.http.read().unwrap_or_else(|e| e.into_inner());
        let mut series: Vec<(String, &Histogram)> = map
            .iter()
            .flat_map(|(route, by)| {
                by.iter().map(move |((method, class), h)| {
                    (
                        format!("route=\"{}\",method=\"{method}\",status=\"{class}xx\"", escape(route)),
                        h,
                    )
                })
            })
            .collect();
        series.sort_by(|a, b| a.0.cmp(&b.0));

        out.push_str("# HELP logb_http_requests_total HTTP requests answered, by matched route template, method and status class.\n");
        out.push_str("# TYPE logb_http_requests_total counter\n");
        for (labels, h) in &series {
            let _ = writeln!(out, "logb_http_requests_total{{{labels}}} {}", h.count.load(Ordering::Relaxed));
        }
        out.push_str("# HELP logb_http_request_duration_seconds Time until the response headers were ready, by matched route template, method and status class.\n");
        out.push_str("# TYPE logb_http_request_duration_seconds histogram\n");
        for (labels, h) in &series {
            h.render(&mut out, "logb_http_request_duration_seconds", labels);
        }
        drop(map);

        // On PostgreSQL `write_db` is the same pool as `db` (see `db::connect_writer`), so both
        // labels describe it; summing across `pool` counts it twice there.
        let pools = [("read", &state.db), ("write", &state.write_db)];
        out.push_str("# HELP logb_db_pool_connections Open connections in the pool, idle or in use.\n");
        out.push_str("# TYPE logb_db_pool_connections gauge\n");
        for (name, pool) in pools {
            let _ = writeln!(out, "logb_db_pool_connections{{pool=\"{name}\"}} {}", pool.size());
        }
        out.push_str("# HELP logb_db_pool_idle_connections Open connections in the pool that are idle.\n");
        out.push_str("# TYPE logb_db_pool_idle_connections gauge\n");
        for (name, pool) in pools {
            let _ = writeln!(out, "logb_db_pool_idle_connections{{pool=\"{name}\"}} {}", pool.num_idle());
        }
        out.push_str("# HELP logb_db_pool_max_connections Most connections the pool will open.\n");
        out.push_str("# TYPE logb_db_pool_max_connections gauge\n");
        for (name, pool) in pools {
            let _ = writeln!(
                out,
                "logb_db_pool_max_connections{{pool=\"{name}\"}} {}",
                pool.options().get_max_connections()
            );
        }

        out.push_str("# HELP logb_db_write_lock_wait_seconds Time a write waited before it held the write lock.\n");
        out.push_str("# TYPE logb_db_write_lock_wait_seconds histogram\n");
        self.write_wait.render(&mut out, "logb_db_write_lock_wait_seconds", "");
        out.push_str("# HELP logb_db_write_lock_timeouts_total Writes answered 503 because the write lock did not come free in time.\n");
        out.push_str("# TYPE logb_db_write_lock_timeouts_total counter\n");
        let _ = writeln!(
            out,
            "logb_db_write_lock_timeouts_total {}",
            self.write_timeouts.load(Ordering::Relaxed)
        );
        out
    }
}

/// A label value as the exposition format wants it: backslash, double quote and newline escaped.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// Middleware counting every request by the route template it matched. Added with
/// `Router::layer`, which is what makes `MatchedPath` visible here: routing has already run.
pub async fn track(State(state): State<App>, req: Request, next: Next) -> Response {
    let started = Instant::now();
    let route = req.extensions().get::<MatchedPath>().cloned();
    let method = method_label(req.method());
    let res = next.run(req).await;
    let route = route.as_ref().map_or(FALLBACK, MatchedPath::as_str);
    state
        .metrics
        .observe_request(route, method, status_class(res.status()), started.elapsed());
    res
}

pub fn router() -> Router<App> {
    Router::new().route("/metrics", get(scrape))
}

/// Compares the two in time independent of where they first differ: both are hashed first, so
/// the comparison is always over 32 bytes whatever their lengths, and every byte is looked at.
fn same_secret(presented: &str, expected: &str) -> bool {
    let a = Sha256::digest(presented.as_bytes());
    let b = Sha256::digest(expected.as_bytes());
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The `Bearer` credential, if the request carries one. The scheme is case-insensitive
/// (RFC 9110 11.1), the credential is not.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, value) = raw.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| value.trim())
}

async fn scrape(State(state): State<App>, headers: HeaderMap) -> Response {
    let Some(expected) = state.config.metrics_token() else {
        return AppError::NotFound.into_response();
    };
    if !bearer(&headers).is_some_and(|p| same_secret(p, expected)) {
        let mut res = AppError::Unauthorized.into_response();
        res.headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return res;
    }
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8")),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        state.metrics.render(&state),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_histogram_renders_cumulative_buckets_and_counts_the_overflow_only_in_inf() {
        let h = Histogram::default();
        h.observe(Duration::from_micros(500)); // <= 0.001
        h.observe(Duration::from_millis(20)); // <= 0.025
        h.observe(Duration::from_secs(60)); // above every bound
        let mut out = String::new();
        h.render(&mut out, "x", "a=\"b\"");
        assert!(out.contains("x_bucket{a=\"b\",le=\"0.001\"} 1\n"), "{out}");
        assert!(out.contains("x_bucket{a=\"b\",le=\"0.01\"} 1\n"), "{out}");
        assert!(out.contains("x_bucket{a=\"b\",le=\"0.025\"} 2\n"), "{out}");
        assert!(out.contains("x_bucket{a=\"b\",le=\"10\"} 2\n"), "{out}");
        assert!(out.contains("x_bucket{a=\"b\",le=\"+Inf\"} 3\n"), "{out}");
        assert!(out.contains("x_count{a=\"b\"} 3\n"), "{out}");
    }

    #[test]
    fn an_unlabelled_histogram_has_no_empty_braces() {
        let h = Histogram::default();
        let mut out = String::new();
        h.render(&mut out, "y", "");
        assert!(out.contains("y_bucket{le=\"+Inf\"} 0\n"), "{out}");
        assert!(out.contains("y_sum 0\n"), "{out}");
        assert!(out.contains("y_count 0\n"), "{out}");
    }

    #[test]
    fn secrets_compare_equal_only_when_equal() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
        assert!(!same_secret("", "abc"));
    }

    #[test]
    fn label_values_are_escaped() {
        assert_eq!(escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
    }
}
