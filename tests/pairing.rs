mod common;
use serde_json::json;

/// A client with no cookie jar: the phone side of pairing never holds a session.
fn bare_client() -> reqwest::Client {
    reqwest::Client::builder().cookie_store(false).build().unwrap()
}

async fn create_pair_code(app: &common::TestApp) -> serde_json::Value {
    let res = app.client.post(app.url("/auth/pair")).send().await.unwrap();
    let status = res.status();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(status, 201, "{body}");
    body
}

#[tokio::test]
async fn creating_a_pair_code_with_a_password_session_returns_code_uri_qr_and_expiry() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let body = create_pair_code(&app).await;
    let code = body["code"].as_str().unwrap();
    assert_eq!(code.len(), 43, "{code}");

    let uri = body["uri"].as_str().unwrap();
    assert!(uri.starts_with("logb://pair?server="), "{uri}");
    assert!(uri.ends_with(&format!("&code={code}")), "{uri}");

    let svg = body["qr_svg"].as_str().unwrap();
    assert!(svg.contains("</svg>"), "{svg}");

    assert!(body["expires_at"].as_str().is_some(), "{body}");
}

/// `SessionUser`, exactly like `create_token`: a bearer token must be refused the same way.
#[tokio::test]
async fn creating_a_pair_code_with_only_an_api_token_is_refused_like_create_token() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let issued = app.client.post(app.url("/auth/tokens")).json(&json!({ "name": "phone" }))
        .send().await.unwrap();
    assert_eq!(issued.status(), 201);
    let issued: serde_json::Value = issued.json().await.unwrap();
    let bearer = issued["token"].as_str().unwrap();

    let anon = bare_client();
    let token_refusal = anon.post(app.url("/auth/tokens")).bearer_auth(bearer)
        .json(&json!({ "name": "another" })).send().await.unwrap();
    assert_eq!(token_refusal.status(), 401, "create_token itself must refuse a bearer token");

    let pair_refusal = anon.post(app.url("/auth/pair")).bearer_auth(bearer).send().await.unwrap();
    assert_eq!(pair_refusal.status(), token_refusal.status(), "pairing must refuse a token the same way create_token does");
}

#[tokio::test]
async fn redeeming_a_code_returns_a_working_token_and_its_owner() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair = create_pair_code(&app).await;

    let anon = bare_client();
    let res = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": "Pixel 8" }))
        .send().await.unwrap();
    let status = res.status();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(status, 200, "{body}");

    let token = body["token"].as_str().unwrap();
    assert!(token.starts_with("logb_pat_"), "{token}");
    assert!(body["token_id"].as_i64().is_some(), "{body}");
    assert_eq!(body["user"]["username"], "ben");
    assert!(body["user"]["id"].as_i64().is_some());

    // The minted token actually works.
    let me = anon.get(app.url("/auth/me")).bearer_auth(token).send().await.unwrap();
    assert_eq!(me.status(), 200);

    // Named as the brief specifies, so it is recognisable in the token list afterwards.
    let listed: Vec<serde_json::Value> = app.client.get(app.url("/auth/tokens"))
        .send().await.unwrap().json().await.unwrap();
    assert!(
        listed.iter().any(|t| t["name"] == "LogB Android · Pixel 8"),
        "no token named for the device: {listed:?}"
    );
}

#[tokio::test]
async fn a_device_name_is_trimmed_and_the_token_name_capped_at_64_characters() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair = create_pair_code(&app).await;

    let anon = bare_client();
    let long_name = "x".repeat(200);
    let res = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": format!("  {long_name}  ") }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);

    let listed: Vec<serde_json::Value> = app.client.get(app.url("/auth/tokens"))
        .send().await.unwrap().json().await.unwrap();
    let name = listed[0]["name"].as_str().unwrap();
    assert_eq!(name.chars().count(), 64, "{name}");
    assert!(name.starts_with("LogB Android · xxx"), "{name}");
}

#[tokio::test]
async fn redeeming_the_same_code_twice_the_second_attempt_is_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair = create_pair_code(&app).await;
    let anon = bare_client();
    let body = json!({ "code": pair["code"], "device_name": "phone" });

    let first = anon.post(app.url("/auth/pair/redeem")).json(&body).send().await.unwrap();
    assert_eq!(first.status(), 200);

    let second = anon.post(app.url("/auth/pair/redeem")).json(&body).send().await.unwrap();
    assert_eq!(second.status(), 401, "{}", second.text().await.unwrap());
}

/// Set directly in the database: this is a code that was never redeemed, just outlived its TTL.
#[tokio::test]
async fn an_expired_code_is_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair = create_pair_code(&app).await;
    sqlx::query("UPDATE pairing_codes SET expires_at = $1")
        .bind("2020-01-01T00:00:00Z")
        .execute(&app.state.db).await.unwrap();

    let anon = bare_client();
    let res = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": "phone" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 401);
}

/// Unknown, expired and already-used codes must be indistinguishable: a caller trying codes
/// must not be able to tell "wrong" apart from "was real but is gone" from the response.
#[tokio::test]
async fn unknown_expired_and_used_codes_all_answer_with_the_same_body() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anon = bare_client();

    // A used code.
    let used_pair = create_pair_code(&app).await;
    let used_body = json!({ "code": used_pair["code"], "device_name": "phone" });
    assert_eq!(anon.post(app.url("/auth/pair/redeem")).json(&used_body).send().await.unwrap().status(), 200);
    let used_again = anon.post(app.url("/auth/pair/redeem")).json(&used_body).send().await.unwrap();
    assert_eq!(used_again.status(), 401);
    let used_again_body: serde_json::Value = used_again.json().await.unwrap();

    // An expired code.
    let expired_pair = create_pair_code(&app).await;
    sqlx::query("UPDATE pairing_codes SET expires_at = $1 WHERE code_hash != $2")
        .bind("2020-01-01T00:00:00Z")
        .bind(logb::domain::pairing::hash(used_pair["code"].as_str().unwrap()))
        .execute(&app.state.db).await.unwrap();
    let expired = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": expired_pair["code"], "device_name": "phone" }))
        .send().await.unwrap();
    assert_eq!(expired.status(), 401);
    let expired_body: serde_json::Value = expired.json().await.unwrap();

    // A code nobody ever issued.
    let unknown = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": "totally-unknown-code-value", "device_name": "phone" }))
        .send().await.unwrap();
    assert_eq!(unknown.status(), 401);
    let unknown_body: serde_json::Value = unknown.json().await.unwrap();

    assert_eq!(used_again_body, expired_body, "used vs expired must read the same");
    assert_eq!(used_again_body, unknown_body, "used vs unknown must read the same");
}

/// The frontend (and the plan) both say a fresh code replaces the old one: showing a new QR
/// must retire whatever was on screen before, even if that older code was never used and has
/// not expired. Without this, a code shown, then hidden by requesting a new one, would go on
/// being a live sign-in nobody watching the Account page could see any more.
#[tokio::test]
async fn a_new_code_invalidates_the_previous_one() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let a = create_pair_code(&app).await;
    let b = create_pair_code(&app).await;
    assert_ne!(a["code"], b["code"]);

    let anon = bare_client();
    let redeem_a = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": a["code"], "device_name": "phone" }))
        .send().await.unwrap();
    assert_eq!(redeem_a.status(), 401, "the superseded code must no longer redeem");

    let redeem_b = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": b["code"], "device_name": "phone" }))
        .send().await.unwrap();
    assert_eq!(redeem_b.status(), 200, "{}", redeem_b.text().await.unwrap());
}

/// An empty `device_name` is refused before the code is touched: the error is a plain 400, in
/// the style of `create_token`'s "name must be 1 to 64 characters", and the code that was not
/// yet spent on it goes on working -- a phone that sends a bad name once should not have burned
/// the QR code the person is still looking at.
#[tokio::test]
async fn an_empty_device_name_is_refused_and_does_not_spend_the_code() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair = create_pair_code(&app).await;
    let anon = bare_client();

    let empty = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": "" }))
        .send().await.unwrap();
    assert_eq!(empty.status(), 400);
    let body: serde_json::Value = empty.json().await.unwrap();
    assert!(body["message"].as_str().unwrap().contains("empty"), "{body}");

    let retry = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": "phone" }))
        .send().await.unwrap();
    assert_eq!(retry.status(), 200, "the refused attempt must not have spent the code: {}", retry.text().await.unwrap());
}

/// Whitespace of any width is not a name: `split_whitespace` treats every Unicode space the
/// same way `trim` already did, so this is not merely the ASCII-space case.
#[tokio::test]
async fn a_whitespace_only_device_name_is_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair = create_pair_code(&app).await;
    let anon = bare_client();

    let res = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": "   \u{2003}\u{00A0}  " }))
        .send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
}

/// A name made only of control characters (category Cc) must not leave a control byte hiding
/// in a token name a person reads in Settings.
#[tokio::test]
async fn a_device_name_of_only_control_characters_is_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair = create_pair_code(&app).await;
    let anon = bare_client();

    let res = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": "\u{0000}\u{0001}\u{0007}" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
}

/// A bidi override does not make it into the stored token name: U+202E can make everything
/// after it in a naive renderer display as if reversed, which is exactly the kind of thing a
/// name from an untrusted device must not be able to do to the token list in Settings.
#[tokio::test]
async fn a_bidi_override_is_stripped_from_the_device_name() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair = create_pair_code(&app).await;
    let anon = bare_client();

    let res = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": "\u{202E}evil.txt" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let listed: Vec<serde_json::Value> = app.client.get(app.url("/auth/tokens"))
        .send().await.unwrap().json().await.unwrap();
    let name = listed[0]["name"].as_str().unwrap();
    assert!(!name.contains('\u{202E}'), "{name}");
    assert_eq!(name, "LogB Android · evil.txt");
}

/// A 200-character name is still capped at 64 characters total, prefix included, and does not
/// panic slicing mid multi-byte character -- exercised with the same all-ASCII shape as A2's
/// original truncation test, now going through the cleaning step first.
#[tokio::test]
async fn a_200_character_device_name_is_still_capped_at_64() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let pair = create_pair_code(&app).await;
    let anon = bare_client();
    let long_name = "p".repeat(200);

    let res = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": pair["code"], "device_name": long_name }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let listed: Vec<serde_json::Value> = app.client.get(app.url("/auth/tokens"))
        .send().await.unwrap().json().await.unwrap();
    let name = listed[0]["name"].as_str().unwrap();
    assert_eq!(name.chars().count(), 64, "{name}");
    assert!(name.starts_with("LogB Android · ppp"), "{name}");
}

/// Redeem has no session and no token of its own, so it is rate-limited by IP exactly like
/// `login` -- otherwise it is a fresh place to brute-force codes from.
#[tokio::test]
async fn too_many_redeem_attempts_from_one_ip_are_rate_limited() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anon = bare_client();
    let body = json!({ "code": "not-a-real-code", "device_name": "phone" });

    // `login_max_attempts` in the test harness's config is 10 (see tests/common/mod.rs).
    for _ in 0..10 {
        let res = anon.post(app.url("/auth/pair/redeem")).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 401);
    }
    let res = anon.post(app.url("/auth/pair/redeem")).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 429);
}
