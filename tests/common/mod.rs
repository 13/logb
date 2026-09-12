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
///
/// Named `_on` because it takes the *server* to make it on: `scratch_database` below is the
/// one that asks no questions and hands back an empty database of whichever backend the suite
/// is running against.
pub async fn scratch_database_on(server_url: &str) -> (ScratchDatabase, String) {
    ScratchDatabase::create(server_url).await
}

/// An empty database of whichever backend the suite is running against, with nothing built on
/// top of it: a temporary file for SQLite, a freshly created database for PostgreSQL.
///
/// It is the same machinery a `TestApp` gets its own database from, for the one kind of test
/// that needs a *second* database rather than a second app -- copying one into another.
pub async fn scratch_database() -> Scratch {
    match test_server_url() {
        Some(server_url) => {
            let (database, url) = ScratchDatabase::create(&server_url).await;
            Scratch { url, _dir: None, _database: Some(database) }
        },
        None => {
            let dir = tempfile::tempdir().unwrap();
            let url = logb::db::sqlite_url(dir.path()).unwrap();
            Scratch { url, _dir: Some(dir), _database: None }
        },
    }
}

/// An empty database and the URL that reaches it. Lives until dropped, exactly as the database
/// behind a `TestApp` does.
pub struct Scratch {
    pub url: String,
    /// The directory the SQLite file lives in, deleted with this handle. `None` on PostgreSQL.
    _dir: Option<tempfile::TempDir>,
    /// The PostgreSQL database, dropped with this handle. `None` on SQLite.
    _database: Option<ScratchDatabase>,
}

impl Scratch {
    /// Every activity paired with the object it hangs off, straight from the database.
    pub async fn all_activity_ids(&self) -> Vec<(i64, i64)> {
        all_activity_ids(&self.url).await
    }

    /// Puts a row into `field_clock`, which references nothing else, so an otherwise empty
    /// database can be given a row without inventing a user to hang it off.
    ///
    /// It leaves `users` empty, so a copy into this database gets past the refusal and runs all
    /// the way to the verification -- which is the only way to exercise what a failed
    /// verification does to the destination.
    pub async fn plant_a_stray_row(&self) {
        // `connect`, not `connect_existing`: a scratch SQLite database is a directory with no
        // file in it yet, and the file has to be made and migrated before it can hold a row.
        let pool = logb::db::connect(&self.url).await.unwrap();
        sqlx::query(
            "INSERT INTO field_clock (entity, entity_uuid, field, edited_at, device_id) \
             VALUES ('object', 'stray', 'name', '2026-01-01T00:00:00Z', 'nobody')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    /// How many rows `users` holds, read through a connection of its own.
    pub async fn user_count(&self) -> i64 {
        let pool = logb::db::connect_existing(&self.url).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&pool).await.unwrap();
        pool.close().await;
        count
    }

    /// Removes one activity, making this database a wrong copy of whatever it was copied from.
    ///
    /// A verification that cannot see this is not verifying anything, so a test needs a way to
    /// break a destination that is otherwise a faithful copy.
    pub async fn delete_one_activity(&self) {
        let pool = logb::db::connect_existing(&self.url).await.unwrap();
        let deleted = sqlx::query("DELETE FROM activities WHERE id = (SELECT min(id) FROM activities)")
            .execute(&pool)
            .await
            .unwrap()
            .rows_affected();
        pool.close().await;
        assert_eq!(deleted, 1, "there was no activity to delete");
    }
}

/// Every `(id, object_id)` in `activities`, in id order, read through a connection of its own.
///
/// Deliberately not through an app's pool: the tests that ask this question have already let go
/// of the database so that a copy could take it, and the answer has to be readable afterwards.
pub async fn all_activity_ids(url: &str) -> Vec<(i64, i64)> {
    let pool = logb::db::connect_existing(url).await.unwrap();
    let rows = sqlx::query_as::<_, (i64, i64)>("SELECT id, object_id FROM activities ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    rows
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

    /// Waits until nothing is connected to this database any more.
    ///
    /// Closing a pool hands the sockets back, but a PostgreSQL backend process lingers in
    /// `pg_stat_activity` for a moment after its client has gone -- and that view is exactly
    /// what `copy::run` reads to decide whether the source is still in use. Asked from the
    /// *server's* own database, so this connection is never one of the ones being counted.
    async fn wait_until_unused(&self) {
        let admin = match admin_pool(&self.server_url).await {
            Ok(admin) => admin,
            Err(e) => {
                eprintln!("could not check whether {} is still in use: {e}", self.name);
                return;
            },
        };
        for _ in 0..200 {
            let busy: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname = $1")
                .bind(&self.name)
                .fetch_one(&admin)
                .await
                .unwrap_or(0);
            if busy == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        admin.close().await;
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
        copy_to: None,
        force: false,
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
    /// The database this app was built on, as a URL -- the same one `--copy-to` would be
    /// pointed at from the command line.
    pub fn database_url(&self) -> String {
        self.state.config.database_url().unwrap()
    }

    /// Lets go of the database, as stopping the server does.
    ///
    /// A copy refuses a source anything else still holds, so a test that copies out of an app's
    /// database has to put the app down first. The app itself stays alive -- it still owns the
    /// data directory and, on PostgreSQL, the scratch database -- it just holds no connections
    /// any more, so anything it is asked to serve after this will fail.
    pub async fn release_database(&self) {
        self.state.db.close().await;
        if let Some(database) = &self._database {
            database.wait_until_unused().await;
        }
    }

    /// Every activity paired with the object it hangs off, straight from the database.
    pub async fn all_activity_ids(&self) -> Vec<(i64, i64)> {
        all_activity_ids(&self.database_url()).await
    }

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

    /// How many rows the change log holds for one `client_op_id`.
    ///
    /// Scoped, where `count_changes` is not, because a test that pushes an op has almost always
    /// created the row it edits over REST first -- and that create logs a change of its own. The
    /// question an idempotency test asks is about ONE op id: how many times did that op land.
    pub async fn count_changes_of(&self, client_op_id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM changes WHERE client_op_id = $1")
            .bind(client_op_id)
            .fetch_one(&self.state.db)
            .await
            .unwrap()
    }

    /// A push body carrying a single `set` of an object's name, under the given `client_op_id`.
    ///
    /// Prepared rather than posted so a caller can send the very same bytes more than once --
    /// which is what a client retrying a push it never saw the answer to actually does.
    ///
    /// `edited_at` is an hour ahead so the op wins last-write-wins against the `field_clock`
    /// the REST create that made this object stamped a moment ago: an op that lost would answer
    /// `superseded`, and this helper is for tests that are about idempotency, not about LWW.
    pub async fn one_set_op(
        &self,
        object: &serde_json::Value,
        name: &str,
        client_op_id: &str,
    ) -> serde_json::Value {
        let edited_at = (chrono::Utc::now() + chrono::Duration::hours(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        self.one_set_op_at(object, name, client_op_id, &edited_at).await
    }

    /// `one_set_op`, but with the caller's own `edited_at` rather than "now plus an hour" --
    /// for a test that is about last-write-wins itself, and so needs to name which of two
    /// edits is the later one rather than let the clock decide.
    pub async fn one_set_op_at(
        &self,
        object: &serde_json::Value,
        name: &str,
        client_op_id: &str,
        edited_at: &str,
    ) -> serde_json::Value {
        let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
            .bind(object["id"].as_i64().expect("an object with an id"))
            .fetch_one(&self.state.db)
            .await
            .unwrap();
        serde_json::json!({ "ops": [{
            "client_op_id": client_op_id, "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": name,
            "edited_at": edited_at, "device_id": "phone"
        }]})
    }

    /// POST /sync/push with a body the caller prepared, answering the raw response so a test
    /// can assert on a status the harness would otherwise have unwrapped away.
    pub async fn push_raw(&self, body: &serde_json::Value) -> reqwest::Response {
        self.client.post(self.url("/sync/push")).json(body).send().await.unwrap()
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

    /// DELETE /objects/{id}: the ordinary REST delete, a tombstone plus the cascade of
    /// tombstones over the object's children, exactly as a user pressing delete produces it.
    pub async fn delete_object(&self, object: &serde_json::Value) {
        let id = object["id"].as_i64().expect("an object id");
        let res = self.client.delete(self.url(&format!("/objects/{id}"))).send().await.unwrap();
        assert_eq!(res.status(), 204, "delete object failed: {}", res.text().await.unwrap());
    }

    /// Backdates every tombstone in the database well past any retention window.
    ///
    /// The window is measured in days, so a test cannot wait one out; this is the only way to
    /// put a row into the state the purge is about. `changes.applied_at` is deliberately left
    /// alone: a test asking whether a row vanished unrecorded needs the log rows that would
    /// have recorded it to still be there to look at.
    pub async fn age_out_tombstones(&self) {
        for table in ["objects", "activities", "reminders", "attachments"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET deleted_at = '2000-01-01T00:00:00Z' WHERE deleted_at IS NOT NULL"
            )))
            .execute(&self.state.db)
            .await
            .unwrap();
        }
    }

    /// Runs the retention purge over this app's database, with the window the server uses.
    pub async fn run_purge(&self) {
        logb::sync::feed::purge(&self.state, 90).await.unwrap();
    }

    /// Activities whose object row is not there any more.
    ///
    /// The weaker half of what a purge test has to check, and it is here to say so: with
    /// `foreign_keys` on, `ON DELETE CASCADE` leaves no orphan -- it leaves nothing at all. A
    /// row destroyed by a cascade is invisible to this count, which is why a test about silent
    /// loss cannot rest on it alone.
    pub async fn count_orphan_activities(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM activities a \
             WHERE NOT EXISTS (SELECT 1 FROM objects o WHERE o.id = a.object_id)",
        )
        .fetch_one(&self.state.db)
        .await
        .unwrap()
    }

    /// The stored `name` of one object, read straight from the database -- so a test asking
    /// which of two concurrent edits won does not also depend on the REST read path.
    pub async fn object_name(&self, object: &serde_json::Value) -> String {
        sqlx::query_scalar("SELECT name FROM objects WHERE id = $1")
            .bind(object["id"].as_i64().expect("an object with an id"))
            .fetch_one(&self.state.db)
            .await
            .unwrap()
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
