mod common;
use serde_json::json;

#[tokio::test]
async fn status_reports_setup_required_until_first_user() {
    let app = common::spawn().await;
    let s: serde_json::Value = reqwest::get(app.url("/auth/status")).await.unwrap().json().await.unwrap();
    assert_eq!(s["setup_required"], true);
    app.setup("ben", "correct horse").await;
    let s: serde_json::Value = reqwest::get(app.url("/auth/status")).await.unwrap().json().await.unwrap();
    assert_eq!(s["setup_required"], false);
}

#[tokio::test]
async fn setup_creates_admin_and_logs_in() {
    let app = common::spawn().await;
    let user = app.setup("ben", "correct horse").await;
    assert_eq!(user["username"], "ben");
    assert_eq!(user["is_admin"], true);
    let me: serde_json::Value = app.client.get(app.url("/auth/me")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["username"], "ben");
}

#[tokio::test]
async fn setup_refused_once_a_user_exists() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = common::new_client()
        .post(app.url("/auth/setup"))
        .json(&json!({ "username": "eve", "password": "password123" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 409);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "conflict");
}

#[tokio::test]
async fn setup_validates_credentials() {
    let app = common::spawn().await;
    for (u, p) in [("ab", "password123"), ("bad name", "password123"), ("ben", "short")] {
        let res = app.client.post(app.url("/auth/setup")).json(&json!({ "username": u, "password": p })).send().await.unwrap();
        assert_eq!(res.status(), 400, "{u}/{p}");
    }
}

#[tokio::test]
async fn login_logout_cycle() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let c = common::new_client();
    assert_eq!(app.client.get(app.url("/auth/me")).send().await.unwrap().status(), 200);
    assert_eq!(c.get(app.url("/auth/me")).send().await.unwrap().status(), 401);

    let res = app.login(&c, "ben", "wrong").await;
    assert_eq!(res.status(), 401);
    let res = app.login(&c, "BEN", "correct horse").await; // username is case-insensitive
    assert_eq!(res.status(), 200);
    assert!(res.headers().get("set-cookie").unwrap().to_str().unwrap().contains("logb_session="));
    assert_eq!(c.get(app.url("/auth/me")).send().await.unwrap().status(), 200);

    assert_eq!(c.post(app.url("/auth/logout")).send().await.unwrap().status(), 204);
    assert_eq!(c.get(app.url("/auth/me")).send().await.unwrap().status(), 401);
}

#[tokio::test]
async fn login_is_rate_limited() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let c = common::new_client();
    for _ in 0..10 {
        assert_eq!(app.login(&c, "ben", "wrong").await.status(), 401);
    }
    assert_eq!(app.login(&c, "ben", "correct horse").await.status(), 429);
}

/// Two setup calls that race past the "is the database empty?" pre-check must not both
/// create an admin: the conditional INSERT lets exactly one through.
///
/// KNOWN TO FAIL ON POSTGRESQL, AND OWNED BY PART TWO OF THE PORT. `INSERT ... WHERE NOT
/// EXISTS (SELECT 1 FROM users)` in `src/api/auth.rs` is atomic only because SQLite has a
/// single writer: under PostgreSQL's MVCC the two statements read the same empty `users` at
/// the same instant, both find nothing, and both insert -- two admins, and the second setup
/// answers 201 where it should answer 409. Left failing on purpose rather than fixed or
/// skipped here: part two covers the whole class of check-then-write races this is one of, and
/// a green test would hide the one case that already has a reproduction. The assertion below
/// says so in its own failure message, so a reader of the output does not have to know that.
#[tokio::test]
async fn concurrent_setup_creates_exactly_one_admin() {
    let app = common::spawn().await;
    let a = common::new_client();
    let b = common::new_client();
    let url = app.url("/auth/setup");
    let (ra, rb) = tokio::join!(
        a.post(&url).json(&json!({ "username": "ben", "password": "correct horse" })).send(),
        b.post(&url).json(&json!({ "username": "eve", "password": "password123" })).send(),
    );
    let (ra, rb) = (ra.unwrap(), rb.unwrap());
    let mut codes = [ra.status().as_u16(), rb.status().as_u16()];
    let winner = if ra.status() == 201 { &a } else { &b };
    codes.sort_unstable();
    // Named in the failure message rather than skipped: see this test's doc comment. On
    // SQLite the note is empty and the assertion reads exactly as it always did.
    let known = known_postgres_race();
    assert_eq!(codes, [201, 409], "exactly one setup may succeed{known}");

    // Only the winner's account exists, and it is the one holding the admin session.
    let users: serde_json::Value = winner.get(app.url("/users")).send().await.unwrap().json().await.unwrap();
    assert_eq!(users.as_array().unwrap().len(), 1, "{users}{known}");
}

/// The cookie that clears the session must carry the same attributes as the one that set it,
/// or a browser can decline to overwrite the live session cookie.
/// The sentence appended to the assertions above when the suite is running on PostgreSQL, so
/// the failure identifies itself as a known, owned one instead of looking like a regression.
fn known_postgres_race() -> &'static str {
    if common::backend() == logb::dialect::Backend::Postgres {
        " -- KNOWN FAILURE ON POSTGRESQL, OWNED BY PART TWO OF THE PORT: `INSERT ... WHERE NOT \
         EXISTS` in src/api/auth.rs is atomic only under SQLite's single writer, so two \
         concurrent first-run setups can both succeed here. Not a regression, and deliberately \
         not fixed in the harness task."
    } else {
        ""
    }
}

#[tokio::test]
async fn logout_cookie_matches_session_cookie_attributes() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/auth/logout")).send().await.unwrap();
    assert_eq!(res.status(), 204);
    let cookie = res.headers().get("set-cookie").unwrap().to_str().unwrap().to_string();
    assert!(cookie.contains("logb_session="), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Lax"), "{cookie}");
    assert!(cookie.contains("Path=/"), "{cookie}");
}

/// `logout-all` ends every session of the caller, including the one making the request.
#[tokio::test]
async fn logout_all_ends_every_session() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let second = common::new_client();
    assert_eq!(app.login(&second, "ben", "correct horse").await.status(), 200);
    assert_eq!(second.get(app.url("/auth/me")).send().await.unwrap().status(), 200);

    assert_eq!(app.client.post(app.url("/auth/logout-all")).send().await.unwrap().status(), 204);
    assert_eq!(app.client.get(app.url("/auth/me")).send().await.unwrap().status(), 401);
    assert_eq!(second.get(app.url("/auth/me")).send().await.unwrap().status(), 401);
    // Signing in again still works.
    assert_eq!(app.login(&second, "ben", "correct horse").await.status(), 200);
}

/// Expired sessions are swept on a timer, not only when someone happens to sign in.
#[tokio::test]
async fn expired_sessions_are_pruned() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind("stale-token").bind(1).bind("2020-01-01T00:00:00Z")
        .execute(&app.state.db).await.unwrap();
    let (before,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions").fetch_one(&app.state.db).await.unwrap();
    assert_eq!(before, 2);

    assert_eq!(logb::tasks::prune_sessions(&app.state).await.unwrap(), 1);
    let (after,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions").fetch_one(&app.state.db).await.unwrap();
    assert_eq!(after, 1, "the live session survives");
    // The live session still works.
    assert_eq!(app.client.get(app.url("/auth/me")).send().await.unwrap().status(), 200);
}
