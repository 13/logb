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
        .json(&json!({ "name": "Golf", "type": "car" })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
}

#[tokio::test]
async fn the_plaintext_is_returned_once_and_never_stored() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (id, token) = issue(&app, "phone").await;

    assert!(token.starts_with("logb_pat_"), "a token should be recognisable on sight: {token}");

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
    let (hash,): (String,) = sqlx::query_as("SELECT token_hash FROM api_tokens WHERE id = $1")
        .bind(id).fetch_one(&app.state.db).await.unwrap();
    assert_ne!(hash, token);
    assert_eq!(hash, logb::files::sha256_hex(token.as_bytes()));
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

/// Minting is the one thing a token may not do at all: a leak that can mint replacements
/// repairs itself faster than it is spotted. Revoking a DIFFERENT token is refused the same
/// way, for the same reason -- self-revoke (below) is the one narrow exception.
#[tokio::test]
async fn a_token_cannot_mint_another_or_revoke_a_different_one() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (id, token) = issue(&app, "phone").await;
    let (other_id, _other_token) = issue(&app, "laptop").await;
    let anon = bare_client();

    let res = anon.post(app.url("/auth/tokens")).bearer_auth(&token)
        .json(&json!({ "name": "another" })).send().await.unwrap();
    assert_eq!(res.status(), 401, "a token must not be able to issue another");

    assert_ne!(id, other_id);
    let res = anon.delete(app.url(&format!("/auth/tokens/{other_id}"))).bearer_auth(&token)
        .send().await.unwrap();
    assert_eq!(res.status(), 401, "nor revoke a token that isn't the one it authenticated with");

    // Reading its own account is still fine -- it is only management that is closed off.
    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(&token).send().await.unwrap().status(), 200);
}

/// The app's sign-out call: `DELETE /api/auth/tokens/{id}` with `Authorization: Bearer <that
/// same token>`. A token may revoke ITSELF -- unlike minting, this cannot be used to leave the
/// owner unable to notice or replace it, since it only ever ends the very credential that was
/// just used, and never touches any other token or session.
#[tokio::test]
async fn a_bearer_token_revokes_itself() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (id, token) = issue(&app, "phone").await;
    let anon = bare_client();
    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(&token).send().await.unwrap().status(), 200);

    let res = anon.delete(app.url(&format!("/auth/tokens/{id}"))).bearer_auth(&token)
        .send().await.unwrap();
    assert_eq!(res.status(), 204, "{}", res.text().await.unwrap());

    // The same bearer, used again, is refused: the revoke took effect immediately.
    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(&token).send().await.unwrap().status(), 401);
}

/// A bearer token trying to revoke a DIFFERENT token of the same user is refused, and that
/// other token is unaffected. Same case as
/// `a_token_cannot_mint_another_or_revoke_a_different_one`, checked here from the angle of "the
/// untouched token still works" rather than "the call was refused".
#[tokio::test]
async fn a_bearer_token_cannot_revoke_a_different_token_of_the_same_user() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_, token_a) = issue(&app, "phone").await;
    let (id_b, token_b) = issue(&app, "laptop").await;
    let anon = bare_client();

    let res = anon.delete(app.url(&format!("/auth/tokens/{id_b}"))).bearer_auth(&token_a)
        .send().await.unwrap();
    assert_eq!(res.status(), 401, "a token may revoke only itself, never a sibling");

    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(&token_b).send().await.unwrap().status(), 200);
    // And the caller's own token, which it did not try to touch, is unaffected too.
    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(&token_a).send().await.unwrap().status(), 200);
}

/// A bearer token trying to revoke another USER's token gets the same refusal as today, not a
/// weaker one just because it is naming an id it happens not to own.
#[tokio::test]
async fn a_bearer_token_cannot_revoke_another_users_token() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (bens_id, _bens_token) = issue(&app, "ben's phone").await;
    let anna = app.create_user_client("anna", "another horse").await;
    let issued = anna.post(app.url("/auth/tokens")).json(&json!({ "name": "anna's phone" }))
        .send().await.unwrap();
    assert_eq!(issued.status(), 201);
    let annas_token = issued.json::<serde_json::Value>().await.unwrap()["token"].as_str().unwrap().to_string();

    let anon = bare_client();
    let res = anon.delete(app.url(&format!("/auth/tokens/{bens_id}"))).bearer_auth(&annas_token)
        .send().await.unwrap();
    assert_eq!(res.status(), 401, "a token may revoke only the token it authenticated with");

    // Ben's token is untouched.
    let listed: Vec<serde_json::Value> = app.client.get(app.url("/auth/tokens"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(listed.len(), 1, "ben's token must still be there: {listed:?}");
}

/// The route's other, older caller: a password session revoking one of its own tokens still
/// works exactly as before self-revoke was added.
#[tokio::test]
async fn a_session_still_revokes_its_own_token() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (id, token) = issue(&app, "phone").await;
    let anon = bare_client();
    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(&token).send().await.unwrap().status(), 200);

    let res = app.client.delete(app.url(&format!("/auth/tokens/{id}"))).send().await.unwrap();
    assert_eq!(res.status(), 204, "{}", res.text().await.unwrap());

    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(&token).send().await.unwrap().status(), 401);
}

/// The app's actual use case end to end: QR sign-in mints a token via `/auth/pair/redeem`
/// (never `POST /auth/tokens`, which stays session-only), and that token can still revoke
/// itself on sign-out just like one issued the ordinary way.
#[tokio::test]
async fn a_redeemed_pairing_token_can_revoke_itself() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair_res = app.client.post(app.url("/auth/pair")).send().await.unwrap();
    assert_eq!(pair_res.status(), 201, "{}", pair_res.text().await.unwrap());
    let pair: serde_json::Value = pair_res.json().await.unwrap();

    let anon = bare_client();
    let redeem_res = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": "phone" }))
        .send().await.unwrap();
    assert_eq!(redeem_res.status(), 200, "{}", redeem_res.text().await.unwrap());
    let redeemed: serde_json::Value = redeem_res.json().await.unwrap();
    let token = redeemed["token"].as_str().unwrap();
    let token_id = redeemed["token_id"].as_i64().unwrap();

    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(token).send().await.unwrap().status(), 200);

    let res = anon.delete(app.url(&format!("/auth/tokens/{token_id}"))).bearer_auth(token)
        .send().await.unwrap();
    assert_eq!(res.status(), 204, "{}", res.text().await.unwrap());

    assert_eq!(anon.get(app.url("/auth/me")).bearer_auth(token).send().await.unwrap().status(), 401);
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
        "Bearer logb_pat_0000000000000000000000000000000000000000000000000000000000000000",
        "Bearer ",
        "Basic logb_pat_x",
        "logb_pat_x",
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

    let (used,): (Option<String>,) = sqlx::query_as("SELECT last_used_at FROM api_tokens WHERE id = $1")
        .bind(id).fetch_one(&app.state.db).await.unwrap();
    assert!(used.is_some(), "using a token should record that it was used");
}

/// CORS exists for a SEPARATE web client on another origin -- the bundled SPA is same-origin and
/// needs none of it, so an instance that has not asked for it must behave exactly as before.
#[tokio::test]
async fn cross_origin_headers_are_absent_until_configured() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let res = app.client.get(app.url("/objects"))
        .header(reqwest::header::ORIGIN, "https://elsewhere.example")
        .send().await.unwrap();
    assert!(
        res.headers().get("access-control-allow-origin").is_none(),
        "no CORS headers should be sent unless origins are configured",
    );
}

/// With an origin configured, the browser is told that origin may call the API -- but never
/// that it may send credentials. Allowing that would let a listed site make the browser attach
/// the session cookie to requests THAT site initiated, which is cross-site request forgery. A
/// cross-origin client authenticates with a bearer token, which travels only because it chose
/// to attach it.
#[tokio::test]
async fn a_configured_origin_is_allowed_but_never_with_credentials() {
    let app = common::spawn_with(|c| c.cors_origins = "https://app.example".into()).await;
    app.setup("ben", "correct horse").await;

    let res = app.client.get(app.url("/objects"))
        .header(reqwest::header::ORIGIN, "https://app.example")
        .send().await.unwrap();
    assert_eq!(res.headers().get("access-control-allow-origin").unwrap(), "https://app.example");
    assert!(
        res.headers().get("access-control-allow-credentials").is_none(),
        "a cross-origin caller must not be able to ride the session cookie",
    );

    // An origin that was not listed is simply not told it may call, whatever it claims to be.
    let res = app.client.get(app.url("/objects"))
        .header(reqwest::header::ORIGIN, "https://elsewhere.example")
        .send().await.unwrap();
    assert!(res.headers().get("access-control-allow-origin").is_none());
}
