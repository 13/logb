#![allow(dead_code)]
//! The harness every integration test spawns its app through.
//!
//! It runs against whichever database the suite is pointed at: nothing set means the SQLite
//! file in a temporary directory that LogB has always used, while `LOGB_TEST_DATABASE_URL`
//! pointing at a PostgreSQL *server* means each test gets a scratch database of its own on it.
//! Either way a test sees an empty, freshly migrated database that no other test can reach.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct TestApp {
    pub base: String,
    pub client: reqwest::Client,
    /// The same shared state the router holds, for tests that drive background work directly.
    pub state: logb::state::App,
    _dir: tempfile::TempDir,
    /// The scratch PostgreSQL database this test owns, dropped with the `TestApp`. `None` on
    /// SQLite, where the temporary directory above is the whole of the cleanup.
    _database: Option<ScratchDatabase>,
}

/// The PostgreSQL server the suite was pointed at, or `None` when it runs on SQLite.
///
/// `LOGB_TEST_DATABASE_URL` is the harness's own variable, deliberately separate from the app's
/// `LOGB_DATABASE_URL`: it names a scratch server the suite may create and drop databases on,
/// not a database an instance serves. A blank value counts as unset, so an exported-but-empty
/// variable does not silently turn the whole suite into a PostgreSQL run that cannot connect.
pub fn test_server_url() -> Option<String> {
    std::env::var("LOGB_TEST_DATABASE_URL").ok().filter(|url| !url.trim().is_empty())
}

/// Which backend the suite is running against.
///
/// Answerable without spawning an app, so a test that is about a mechanism only one database
/// has can decide before it builds anything. `TestApp::state.backend` says the same thing for
/// tests that already have an app.
pub fn backend() -> logb::dialect::Backend {
    match test_server_url() {
        Some(url) => logb::dialect::Backend::of(&url),
        None => logb::dialect::Backend::Sqlite,
    }
}

/// Says, out loud, that a named test is standing down on this backend, and why.
///
/// Deliberately a helper that *reports* rather than one that decides: each caller passes its
/// own reason, so "this test is SQLite-only" stays a per-test judgement with a sentence behind
/// it rather than a blanket skip. The line is printed rather than swallowed, so a run with
/// `--nocapture` shows which tests stood down and why instead of them quietly reporting `ok`.
pub fn skipped_on_postgres(test: &str, why: &str) -> bool {
    if backend() == logb::dialect::Backend::Postgres {
        eprintln!("SKIPPED on PostgreSQL: {test} -- {why}");
        return true;
    }
    false
}

/// A name no other test in this run will pick: the process id, so two test binaries running at
/// once cannot collide, and a counter, so two tests in the same binary cannot either.
///
/// Deliberately not a real uuid -- uniqueness within a run is all this needs, and a uuid crate
/// would be a dependency the app itself does not have.
pub(crate) fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("{}_{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Swaps the database name in a server URL, keeping user, password, host, port and query.
///
/// `postgres://user:pw@host:5432/postgres` becomes `postgres://user:pw@host:5432/logb_test_1_0`.
/// Hand-written rather than parsed with a URL crate for the same reason as above: the shape is
/// fixed and a dependency is not worth it.
pub(crate) fn replace_database_in_url(server_url: &str, name: &str) -> String {
    let (scheme, rest) = match server_url.split_once("://") {
        Some((scheme, rest)) => (scheme, rest),
        // No authority at all: nothing to keep but the scheme.
        None => return format!("{server_url}/{name}"),
    };
    // The authority ends at the first `/`, `?` or `#`; everything from the `?` on is query and
    // fragment, which carry connection options and must survive the swap.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let suffix = match tail.find(['?', '#']) {
        Some(at) => &tail[at..],
        None => "",
    };
    format!("{scheme}://{authority}/{name}{suffix}")
}

/// A single connection to the server itself, for the statements that cannot be run from inside
/// the database they are about.
async fn admin_pool(server_url: &str) -> Result<sqlx::AnyPool, sqlx::Error> {
    sqlx::any::install_default_drivers();
    sqlx::any::AnyPoolOptions::new().max_connections(1).connect(server_url).await
}

/// Every scratch database this harness has ever made shares this prefix, so leftovers from a
/// run that was killed can be recognised and swept.
const SCRATCH_PREFIX: &str = "logb_test_";

/// One test's own PostgreSQL database.
///
/// Each test gets one, created from the server URL and dropped when the test finishes. The
/// suite runs in parallel and every test assumes it is alone -- sharing one database would make
/// failures depend on which tests happened to run together, which is the worst kind of flake to
/// debug.
pub struct ScratchDatabase {
    server_url: String,
    name: String,
}

/// A scratch database with no app around it, and the URL that reaches it, for a test that
/// wants a database rather than a whole instance. It lives until the returned handle is
/// dropped, exactly as a `TestApp`'s does.
pub async fn scratch_database(server_url: &str) -> (ScratchDatabase, String) {
    ScratchDatabase::create(server_url).await
}

impl ScratchDatabase {
    async fn create(server_url: &str) -> (Self, String) {
        let admin = admin_pool(server_url)
            .await
            .unwrap_or_else(|e| panic!("LOGB_TEST_DATABASE_URL is set but unreachable: {e}"));
        sweep_leftovers(&admin).await;
        let name = format!("{SCRATCH_PREFIX}{}", unique_suffix());
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .unwrap_or_else(|e| panic!("could not create the scratch database {name}: {e}"));
        admin.close().await;
        let url = replace_database_in_url(server_url, &name);
        (Self { server_url: server_url.to_string(), name }, url)
    }
}

impl Drop for ScratchDatabase {
    /// Teardown has to happen from a `Drop`, which cannot await, and the test's own runtime is
    /// being torn down around it -- so this runs on a thread of its own with a runtime of its
    /// own, and is joined so the database is gone before the test returns.
    ///
    /// `WITH (FORCE)` because the app's pool, and the server task holding it, are still alive
    /// at this point: PostgreSQL refuses to drop a database anything is still connected to.
    fn drop(&mut self) {
        let (server_url, name) = (self.server_url.clone(), self.name.clone());
        let dropped = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let admin = admin_pool(&server_url).await?;
                let sql = format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)");
                let result = sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(&admin).await;
                admin.close().await;
                result.map(|_| ())
            })
        })
        .join();
        // A failure here leaks a database rather than failing a test that has already passed
        // or failed on its own merits; `sweep_leftovers` collects it on the next run. Reported
        // rather than swallowed, so a server that is quietly filling up says so.
        if let Ok(Err(e)) = dropped {
            eprintln!("could not drop the scratch database {}: {e}", self.name);
        }
    }
}

/// Drops scratch databases left behind by runs that were killed before their teardown ran.
///
/// Once per process, and never touching this process's own databases. A database another run
/// is still using cannot be dropped without `FORCE`, which is deliberately not used here: the
/// plain `DROP` fails, the error is ignored, and the concurrent run keeps its database.
async fn sweep_leftovers(admin: &sqlx::AnyPool) {
    static SWEPT: AtomicBool = AtomicBool::new(false);
    if SWEPT.swap(true, Ordering::SeqCst) {
        return;
    }
    let mine = format!("{SCRATCH_PREFIX}{}_", std::process::id());
    // `datname` is PostgreSQL's `name` type, which the `Any` driver cannot decode -- without
    // the cast this query fails and the sweep silently does nothing.
    let names = sqlx::query_scalar::<_, String>(
        "SELECT datname::text FROM pg_database WHERE datname LIKE $1 AND datname NOT LIKE $2",
    )
    .bind(format!("{SCRATCH_PREFIX}%"))
    .bind(format!("{mine}%"))
    .fetch_all(admin)
    .await;
    let names = match names {
        Ok(names) => names,
        // A sweep that cannot run is not a reason to fail a test: the databases it would have
        // collected are stale, not in the way. Reported so it cannot rot unnoticed.
        Err(e) => {
            eprintln!("could not sweep leftover scratch databases: {e}");
            return;
        },
    };
    for name in names {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP DATABASE {name}")))
            .execute(admin)
            .await;
    }
}

pub fn test_config(data_dir: std::path::PathBuf) -> logb::config::Config {
    logb::config::Config {
        data_dir,
        bind: "127.0.0.1".into(),
        port: 0,
        max_upload_mb: 2,
        max_import_mb: 4,
        notify_url: None,
        notify_hour: 8,
        notify_format: "json".into(),
        timezone: chrono_tz::Tz::UTC,
        backup: None,
        backup_dir: None,
        backup_hour: 3,
        restore: None,
        healthcheck: false,
        secure_cookie: "false".into(),
        log: "warn".into(),
        trust_proxy: false,
        login_max_attempts: 10,
        cors_origins: String::new(),
        database_url: None,
        // Each test spawns its own app and pool, and the suite runs many of them in parallel;
        // a pool of 16 per test (today's PostgreSQL default) exhausts a stock server's
        // `max_connections` long before the suite finishes. A small pool is all one test needs.
        db_pool_size: Some(2),
    }
}

pub async fn spawn() -> TestApp {
    spawn_with(|_| {}).await
}

/// As `spawn`, with a chance to adjust the config before the app is built.
pub async fn spawn_with(tweak: impl FnOnce(&mut logb::config::Config)) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(dir.path().to_path_buf());
    // On SQLite the config's own data directory names the database, exactly as it always has.
    // On PostgreSQL the test gets a scratch database of its own, named in `database_url` --
    // the data directory is still where its blobs go.
    let database = match test_server_url() {
        Some(server_url) => {
            let (database, url) = ScratchDatabase::create(&server_url).await;
            config.database_url = Some(url);
            Some(database)
        },
        None => None,
    };
    // The tweak runs last so a test can still override anything, including the database URL.
    tweak(&mut config);
    let (app, state) = logb::build_with_state(config).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    });
    TestApp {
        base: format!("http://{addr}/api"),
        client: new_client(),
        state,
        _dir: dir,
        _database: database,
    }
}

/// Percent-encodes a query-string value. The search terms that matter here are accented, and
/// a raw `ö` in a URL is not something the server is obliged to accept.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A fresh client with its own cookie jar (a second "browser").
pub fn new_client() -> reqwest::Client {
    reqwest::Client::builder().cookie_store(true).build().unwrap()
}

impl TestApp {
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// POST /auth/setup with the given credentials using `self.client` (first user = admin).
    pub async fn setup(&self, username: &str, password: &str) -> serde_json::Value {
        let res = self
            .client
            .post(self.url("/auth/setup"))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201, "setup failed: {}", res.text().await.unwrap());
        res.json().await.unwrap()
    }

    /// POST /auth/login with an arbitrary client.
    pub async fn login(&self, client: &reqwest::Client, username: &str, password: &str) -> reqwest::Response {
        client
            .post(self.url("/auth/login"))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await
            .unwrap()
    }

    /// Admin (self.client) creates a user and returns a logged-in client for it.
    pub async fn create_user_client(&self, username: &str, password: &str) -> reqwest::Client {
        let res = self
            .client
            .post(self.url("/users"))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201, "create user failed: {}", res.text().await.unwrap());
        let c = new_client();
        let res = self.login(&c, username, password).await;
        assert_eq!(res.status(), 200);
        c
    }

    /// Create an object with self.client; returns its JSON.
    pub async fn create_object(&self, client: &reqwest::Client, name: &str, unit: Option<&str>) -> serde_json::Value {
        let res = client
            .post(self.url("/objects"))
            .json(&serde_json::json!({
                "name": name, "type": "car", "counter_unit": unit,
                "description": "", "purchase_date": null, "purchase_price_cents": null
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201, "create object failed: {}", res.text().await.unwrap());
        res.json().await.unwrap()
    }

    /// POST /users as the admin, returning the raw response so a caller can assert on a
    /// rejection as easily as on a success.
    pub async fn create_user(&self, username: &str, password: &str) -> reqwest::Response {
        self.client
            .post(self.url("/users"))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await
            .unwrap()
    }

    /// POST /auth/login from a fresh browser, returning the raw response.
    pub async fn sign_in(&self, username: &str, password: &str) -> reqwest::Response {
        self.login(&new_client(), username, password).await
    }

    /// Adds an activity to an object, taking the id straight out of `create_object`'s JSON.
    pub async fn create_activity(&self, object_id: &serde_json::Value, title: &str) -> serde_json::Value {
        let id = object_id.as_i64().expect("an object id");
        let res = self
            .client
            .post(self.url(&format!("/objects/{id}/activities")))
            .json(&serde_json::json!({
                "date": "2024-03-01", "category": "maintenance", "title": title, "notes": ""
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201, "create activity failed: {}", res.text().await.unwrap());
        res.json().await.unwrap()
    }

    /// GET /sync/pull from a cursor, exactly as a device following the log does.
    ///
    /// The epoch travels with every cursor past the first, and it is constant for the life of
    /// one database -- so this reads it from the app's own settings rather than making every
    /// caller thread it back out of the previous page.
    pub async fn pull(&self, since: i64) -> serde_json::Value {
        let epoch = logb::sync::epoch::current(&self.state.db).await.unwrap();
        let res = self
            .client
            .get(self.url(&format!("/sync/pull?since={since}&epoch={epoch}")))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "pull failed: {}", res.text().await.unwrap());
        res.json().await.unwrap()
    }

    /// How many rows the change log holds, read straight from the database.
    ///
    /// This is the number a puller that missed nothing must have seen, and it deliberately does
    /// not go through the API: the question is what the log contains, not what it serves.
    pub async fn count_changes(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM changes")
            .fetch_one(&self.state.db)
            .await
            .unwrap()
    }

    /// GET /search, with the term encoded by the client rather than pasted into the URL --
    /// the terms that matter here are accented.
    pub async fn search(&self, term: &str) -> serde_json::Value {
        let res = self
            .client
            .get(self.url(&format!("/search?q={}", percent_encode(term))))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "search failed: {}", res.text().await.unwrap());
        res.json().await.unwrap()
    }

    /// The caller's unarchived object names, in the order the API returns them.
    pub async fn object_names(&self) -> Vec<String> {
        let res = self.client.get(self.url("/objects")).send().await.unwrap();
        assert_eq!(res.status(), 200, "list objects failed: {}", res.text().await.unwrap());
        let rows: serde_json::Value = res.json().await.unwrap();
        rows.as_array()
            .unwrap()
            .iter()
            .map(|o| o["name"].as_str().unwrap().to_string())
            .collect()
    }
}
