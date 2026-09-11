#![allow(dead_code)]
use std::net::SocketAddr;

pub struct TestApp {
    pub base: String,
    pub client: reqwest::Client,
    /// The same shared state the router holds, for tests that drive background work directly.
    pub state: logb::state::App,
    _dir: tempfile::TempDir,
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
    }
}

pub async fn spawn() -> TestApp {
    spawn_with(|_| {}).await
}

/// As `spawn`, with a chance to adjust the config before the app is built.
pub async fn spawn_with(tweak: impl FnOnce(&mut logb::config::Config)) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(dir.path().to_path_buf());
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
