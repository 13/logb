#![allow(dead_code)]
use std::net::SocketAddr;

pub struct TestApp {
    pub base: String,
    pub client: reqwest::Client,
    _dir: tempfile::TempDir,
}

pub async fn spawn() -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let config = memto::config::Config {
        data_dir: dir.path().to_path_buf(),
        bind: "127.0.0.1".into(),
        port: 0,
        max_upload_mb: 2,
        secure_cookie: "false".into(),
        log: "warn".into(),
        trust_proxy: false,
    };
    let app = memto::build(config).await.unwrap();
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
