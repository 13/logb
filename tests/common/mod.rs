#![allow(dead_code)]
use std::net::SocketAddr;

pub struct TestApp {
    pub base: String,
    pub client: reqwest::Client,
    /// The same shared state the router holds, for tests that drive background work directly.
    pub state: logby::state::App,
    _dir: tempfile::TempDir,
}

pub fn test_config(data_dir: std::path::PathBuf) -> logby::config::Config {
    logby::config::Config {
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
        healthcheck: false,
        secure_cookie: "false".into(),
        log: "warn".into(),
        trust_proxy: false,
        login_max_attempts: 10,
        cors_origins: String::new(),
    }
}

pub async fn spawn() -> TestApp {
    spawn_with(|_| {}).await
}

/// As `spawn`, with a chance to adjust the config before the app is built.
pub async fn spawn_with(tweak: impl FnOnce(&mut logby::config::Config)) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(dir.path().to_path_buf());
    tweak(&mut config);
    let (app, state) = logby::build_with_state(config).await.unwrap();
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
                "name": name, "category": "car", "counter_unit": unit,
                "description": "", "purchase_date": null, "purchase_price_cents": null
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 201, "create object failed: {}", res.text().await.unwrap());
        res.json().await.unwrap()
    }
}
