use crate::common;
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

/// Logs in from a proxy-reported address of the caller's choosing, so one test can play many
/// different clients against a `trust_proxy` instance.
async fn login_from(app: &common::TestApp, ip: &str, username: &str, password: &str) -> reqwest::StatusCode {
    common::new_client()
        .post(app.url("/auth/login"))
        .header("x-forwarded-for", ip)
        .json(&json!({ "username": username, "password": password }))
        .send()
        .await
        .unwrap()
        .status()
}

/// The per-IP limit alone does nothing against guesses spread across many addresses -- a
/// botnet, or anyone behind a proxy that reports a different hop per request. The same
/// username is limited on its own, however its caller spells its case, and a different account
/// is untouched by it.
#[tokio::test]
async fn login_is_rate_limited_per_username_across_addresses() {
    let app = common::spawn_with(|c| c.trust_proxy = true).await;
    app.setup("ben", "correct horse").await;
    app.create_user_client("anna", "password123").await;
    for i in 0..10 {
        let name = if i % 2 == 0 { "ben" } else { "BEN" };
        assert_eq!(login_from(&app, &format!("198.51.100.{i}"), name, "wrong").await, 401, "attempt {i}");
    }
    assert_eq!(login_from(&app, "198.51.100.200", "Ben", "correct horse").await, 429);
    assert_eq!(login_from(&app, "198.51.100.201", "anna", "password123").await, 200);
}

/// An unknown username is limited exactly like a real one: if only real accounts ever
/// answered 429, the limit itself would say which names exist.
#[tokio::test]
async fn an_unknown_username_is_rate_limited_like_a_real_one() {
    let app = common::spawn_with(|c| c.trust_proxy = true).await;
    app.setup("ben", "correct horse").await;
    for i in 0..10 {
        assert_eq!(login_from(&app, &format!("198.51.100.{i}"), "nobody", "wrong").await, 401);
    }
    assert_eq!(login_from(&app, "198.51.100.200", "nobody", "wrong").await, 429);
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
    // Raced several times, because once is not decisive: run alone this fails on PostgreSQL
    // every time, but under a loaded parallel suite the two requests sometimes serialise by
    // luck and it passed 2 runs in 3. An intermittently red test is worse than a red one --
    // it gets rerun until it is green and then believed -- so the race is repeated until it
    // either misbehaves or has had enough chances that a pass means something.
    for round in 1..=5 {
        race_one_setup(round).await;
    }
}

async fn race_one_setup(round: u32) {
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
    assert_eq!(codes, [201, 409], "round {round}: exactly one setup may succeed{known}");

    // Only the winner's account exists, and it is the one holding the admin session.
    let users: serde_json::Value = winner.get(app.url("/users")).send().await.unwrap().json().await.unwrap();
    assert_eq!(users.as_array().unwrap().len(), 1, "round {round}: {users}{known}");
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

/// Signing in or out tells the browser to drop its HTTP cache, so files a shared browser
/// cached under the old `immutable` header (pre-3555016) can't outlive the session that
/// fetched them. Never `"storage"` (that would also wipe the offline outbox and
/// `localStorage`) and never absent on a failed login, which changes nothing worth clearing.
#[tokio::test]
async fn sign_in_and_out_clear_the_browsers_http_cache() {
    let app = common::spawn().await;

    // setup (first admin, signed in on the spot).
    let res = common::new_client()
        .post(app.url("/auth/setup"))
        .json(&json!({ "username": "ben", "password": "correct horse" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    assert_eq!(res.headers().get("clear-site-data").unwrap().to_str().unwrap(), "\"cache\"");

    let c = common::new_client();
    let res = app.login(&c, "ben", "wrong").await;
    assert_eq!(res.status(), 401);
    assert!(res.headers().get("clear-site-data").is_none(), "a failed login must not clear anything");

    let res = app.login(&c, "ben", "correct horse").await;
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers().get("clear-site-data").unwrap().to_str().unwrap(), "\"cache\"");

    let res = c.post(app.url("/auth/logout")).send().await.unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(res.headers().get("clear-site-data").unwrap().to_str().unwrap(), "\"cache\"");

    assert_eq!(app.login(&c, "ben", "correct horse").await.status(), 200);
    let res = c.post(app.url("/auth/logout-all")).send().await.unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(res.headers().get("clear-site-data").unwrap().to_str().unwrap(), "\"cache\"");
}

/// Expired sessions are swept on a timer, not only when someone happens to sign in.
#[tokio::test]
async fn expired_sessions_are_pruned() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, CAST($3 AS TIMESTAMP))")
        .bind("stale-token").bind(1)
        .bind("2020-01-01 00:00:00")
        .execute(&app.state.db).await.unwrap();
    let (before,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions").fetch_one(&app.state.db).await.unwrap();
    assert_eq!(before, 2);

    assert_eq!(logb::tasks::prune_sessions(&app.state).await.unwrap(), 1);
    let (after,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions").fetch_one(&app.state.db).await.unwrap();
    assert_eq!(after, 1, "the live session survives");
    // The live session still works.
    assert_eq!(app.client.get(app.url("/auth/me")).send().await.unwrap().status(), 200);
}

/// The database holds a hash of each session token, never the token: a copy of the database
/// -- a backup, a snapshot, a stolen disk -- must not be a stack of cookies anyone can replay.
/// The row a sign-in writes is found only by hashing the cookie the browser was given, and the
/// stored value itself does not work as a cookie.
#[tokio::test]
async fn session_tokens_are_stored_hashed() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.login(&common::new_client(), "ben", "correct horse").await;
    assert_eq!(res.status(), 200);
    let cookie = res.headers().get("set-cookie").unwrap().to_str().unwrap().to_string();
    let token = cookie.split(';').next().unwrap().strip_prefix("logb_session=").unwrap().to_string();

    let stored: Vec<(String,)> = sqlx::query_as("SELECT token FROM sessions").fetch_all(&app.state.db).await.unwrap();
    assert!(stored.iter().all(|(t,)| *t != token), "the plaintext token is in the database");
    let hashed = logb::files::sha256_hex(token.as_bytes());
    assert!(stored.iter().any(|(t,)| *t == hashed), "no row holds sha256(token): {stored:?}");

    // The hash is not itself a credential.
    let replay = reqwest::Client::new()
        .get(app.url("/auth/me"))
        .header("cookie", format!("logb_session={hashed}"))
        .send().await.unwrap();
    assert_eq!(replay.status(), 401);
    // The real token still is, and logging out with it removes its row.
    let c = reqwest::Client::new();
    let me = c.get(app.url("/auth/me")).header("cookie", format!("logb_session={token}")).send().await.unwrap();
    assert_eq!(me.status(), 200);
    let out = c.post(app.url("/auth/logout")).header("cookie", format!("logb_session={token}")).send().await.unwrap();
    assert_eq!(out.status(), 204);
    let (left,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions WHERE token = $1")
        .bind(&hashed).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(left, 0, "logout must delete the hashed row");
}
