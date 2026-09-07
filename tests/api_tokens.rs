mod common;
use serde_json::json;

// A client that is not a browser should not have to pretend to be one: a cookie's SameSite and
// HttpOnly attributes exist to constrain a browser, and nothing outside one benefits from
// either. These pin the bearer-token path a native or scripted client uses instead.

async fn issue(app: &common::TestApp, name: &str) -> (i64, String) {
    let res = app.client.post(app.url("/auth/tokens")).json(&json!({ "name": name }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    (body["id"].as_i64().unwrap(), body["token"].as_str().unwrap().to_string())
}

/// A fresh client with no cookie jar at all: the only thing it can present is the header.
fn bare_client() -> reqwest::Client {
    reqwest::Client::builder().cookie_store(false).build().unwrap()
}

#[tokio::test]
async fn a_token_authenticates_a_client_with_no_cookies_at_all() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_, token) = issue(&app, "phone").await;

    let anon = bare_client();
    // Without it, the same request is refused -- so the header is doing the work, not some
    // residual session.
    let res = anon.get(app.url("/objects")).send().await.unwrap();
    assert_eq!(res.status(), 401);

    let res = anon.get(app.url("/objects")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    // And it can write, not merely read.
    let res = anon.post(app.url("/objects")).bearer_auth(&token)
        .json(&json!({ "name": "Golf", "category": "vehicle" })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
}

#[tokio::test]
async fn the_plaintext_is_returned_once_and_never_stored() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (id, token) = issue(&app, "phone").await;

    assert!(token.starts_with("memto_pat_"), "a token should be recognisable on sight: {token}");

    let listed: Vec<serde_json::Value> = app.client.get(app.url("/auth/tokens"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], id);
    assert_eq!(listed[0]["name"], "phone");
    assert!(listed[0].get("token").is_none(), "the listing must never carry the plaintext");
    let prefix = listed[0]["prefix"].as_str().unwrap();
    assert!(token.starts_with(prefix), "the prefix should identify the token the user is holding");
    assert!(prefix.len() < token.len(), "and must not be the whole thing");

    // Nor is it in the database: a leaked backup must not hand over the account.
    let (hash,): (String,) = sqlx::query_as("SELECT token_hash FROM api_tokens WHERE id = ?")
        .bind(id).fetch_one(&app.state.db).await.unwrap();
    assert_ne!(hash, token);
    assert_eq!(hash, memto::files::sha256_hex(token.as_bytes()));
}

#[tokio::test]
async fn a_revoked_token_stops_working_immediately() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (id, token) = issue(&app, "phone").await;
    let anon = bare_client();
    assert_eq!(anon.get(app.url("/objects")).bearer_auth(&token).send().await.unwrap().status(), 200);

    let res = app.client.delete(app.url(&format!("/auth/tokens/{id}"))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    assert_eq!(anon.get(app.url("/objects")).bearer_auth(&token).send().await.unwrap().status(), 401);
    let listed: Vec<serde_json::Value> = app.client.get(app.url("/auth/tokens"))
        .send().await.unwrap().json().await.unwrap();
    assert!(listed.is_empty());
}

/// Managing tokens is the one thing a token may not do: a leak that can mint replacements, and
/// revoke the ones its owner would use to notice, repairs itself faster than it is spotted.
#[tokio::test]
async fn a_token_cannot_mint_or_revoke_tokens() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (id, token) = issue(&app, "phone").await;
    let anon = bare_client();

    let res = anon.post(app.url("/auth/tokens")).bearer_auth(&token)
        .json(&json!({ "name": "another" })).send().await.unwrap();
    assert_eq!(res.status(), 401, "a token must not be able to issue another");

    let res = anon.delete(app.url(&format!("/auth/tokens/{id}"))).bearer_auth(&token)
        .send().await.unwrap();
    assert_eq!(res.status(), 401, "nor revoke one");

    // Reading its own account is still fine -- it is only management that is closed off.
    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(&token).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn tokens_are_scoped_to_their_owner() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (bens_id, bens_token) = issue(&app, "ben's phone").await;
    let anna = app.create_user_client("anna", "another horse").await;

    let listed: Vec<serde_json::Value> = anna.get(app.url("/auth/tokens"))
        .send().await.unwrap().json().await.unwrap();
    assert!(listed.is_empty(), "anna must not see ben's tokens");

    let res = anna.delete(app.url(&format!("/auth/tokens/{bens_id}"))).send().await.unwrap();
    assert_eq!(res.status(), 404, "nor revoke one; whether the id exists is not her business");

    // Ben's token still works, and reaches only ben's data.
    let anon = bare_client();
    let res = anon.get(app.url("/objects")).bearer_auth(&bens_token).send().await.unwrap();
    assert_eq!(res.status(), 200);
}

/// A password reset is taken precisely because an account may be compromised. Revoking sessions
/// but not tokens would cancel the credential the owner can see and keep the one an attacker
/// actually took -- and unlike a session, a token never expires on its own.
#[tokio::test]
async fn changing_a_password_revokes_every_token() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_, token) = issue(&app, "phone").await;
    let anon = bare_client();
    assert_eq!(anon.get(app.url("/objects")).bearer_auth(&token).send().await.unwrap().status(), 200);

    let me: serde_json::Value = app.client.get(app.url("/auth/me")).send().await.unwrap().json().await.unwrap();
    let res = app.client.patch(app.url(&format!("/users/{}", me["id"].as_i64().unwrap())))
        .json(&json!({ "password": "a different horse" })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    assert_eq!(
        anon.get(app.url("/objects")).bearer_auth(&token).send().await.unwrap().status(),
        401,
        "a password change must end API access too, not only browser sessions",
    );
}

#[tokio::test]
async fn a_bad_or_malformed_credential_is_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anon = bare_client();

    for header in [
        "Bearer memto_pat_0000000000000000000000000000000000000000000000000000000000000000",
        "Bearer ",
        "Basic memto_pat_x",
        "memto_pat_x",
        "",
    ] {
        let res = anon.get(app.url("/objects"))
            .header(reqwest::header::AUTHORIZATION, header)
            .send().await.unwrap();
        assert_eq!(res.status(), 401, "should have been refused: {header:?}");
    }
}

#[tokio::test]
async fn a_name_is_required_and_bounded() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    for bad in [json!({ "name": "" }), json!({ "name": "   " }), json!({ "name": "x".repeat(65) })] {
        let res = app.client.post(app.url("/auth/tokens")).json(&bad).send().await.unwrap();
        assert_eq!(res.status(), 400, "{bad}");
    }
}

#[tokio::test]
async fn use_is_recorded_so_a_forgotten_token_can_be_recognised() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (id, token) = issue(&app, "phone").await;

    let listed: Vec<serde_json::Value> = app.client.get(app.url("/auth/tokens"))
        .send().await.unwrap().json().await.unwrap();
    assert!(listed[0]["last_used_at"].is_null(), "never used yet");

    bare_client().get(app.url("/objects")).bearer_auth(&token).send().await.unwrap();

    let (used,): (Option<String>,) = sqlx::query_as("SELECT last_used_at FROM api_tokens WHERE id = ?")
        .bind(id).fetch_one(&app.state.db).await.unwrap();
    assert!(used.is_some(), "using a token should record that it was used");
}
