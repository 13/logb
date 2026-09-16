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

/// A reverse proxy may append to `X-Forwarded-Host` rather than replace it -- the same shape
/// `X-Forwarded-For` can take -- so only the first, trimmed entry must end up in the pairing
/// URI, exactly as `auth::client_ip` already does for the forwarded client address.
#[tokio::test]
async fn x_forwarded_host_takes_only_the_first_of_a_comma_separated_list() {
    let app = common::spawn_with(|c| c.trust_proxy = true).await;
    app.setup("ben", "correct horse").await;

    let res = app.client.post(app.url("/auth/pair"))
        .header("x-forwarded-host", "first.example.com, second.example.com")
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let body: serde_json::Value = res.json().await.unwrap();
    let uri = body["uri"].as_str().unwrap();
    assert!(uri.contains("first.example.com"), "{uri}");
    assert!(!uri.contains("second.example.com"), "{uri}");
}

/// Whichever header supplies the host that lands in the pairing URI, it must first parse as a
/// bare `host[:port]` authority: a value with an embedded `/` or space is not a host, it is an
/// attempt to smuggle extra path or query into a URI this browser is about to render as a link
/// and a QR code.
#[tokio::test]
async fn an_invalid_forwarded_host_is_refused_with_400() {
    let app = common::spawn_with(|c| c.trust_proxy = true).await;
    app.setup("ben", "correct horse").await;

    let res = app.client.post(app.url("/auth/pair"))
        .header("x-forwarded-host", "not a host/with a slash")
        .send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
}

/// The same check applies to the plain `Host` header, which is what a deployment with no
/// trusted proxy in front of it falls back to.
#[tokio::test]
async fn an_invalid_host_header_is_refused_with_400() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let res = app.client.post(app.url("/auth/pair"))
        .header(reqwest::header::HOST, "not a valid host")
        .send().await.unwrap();
    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
}

/// `create_pair` takes only a session cookie and no JSON body -- exactly what a plain
/// cross-site `<form method=post>` can hit -- so a `Sec-Fetch-Site` that actively says the
/// request crossed a site boundary must be refused.
#[tokio::test]
async fn a_same_origin_sec_fetch_site_is_allowed_but_cross_site_is_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let same_origin = app.client.post(app.url("/auth/pair"))
        .header("sec-fetch-site", "same-origin")
        .send().await.unwrap();
    assert_eq!(same_origin.status(), 201, "{}", same_origin.text().await.unwrap());

    let cross_site = app.client.post(app.url("/auth/pair"))
        .header("sec-fetch-site", "cross-site")
        .send().await.unwrap();
    assert_eq!(cross_site.status(), 403, "{}", cross_site.text().await.unwrap());
}

/// Both responses on this router carry a secret -- a code that signs a device in, or the token
/// it was redeemed for -- and neither may be replayed from a cache.
#[tokio::test]
async fn create_pair_and_redeem_answer_cache_control_no_store() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let created = app.client.post(app.url("/auth/pair")).send().await.unwrap();
    assert_eq!(created.status(), 201);
    assert_eq!(created.headers().get("cache-control").unwrap(), "no-store");
    let created: serde_json::Value = created.json().await.unwrap();

    let anon = bare_client();
    let redeemed = anon.post(app.url("/auth/pair/redeem"))
        .json(&json!({ "code": created["code"], "device_name": "phone" }))
        .send().await.unwrap();
    assert_eq!(redeemed.status(), 200, "{}", redeemed.text().await.unwrap());
    assert_eq!(redeemed.headers().get("cache-control").unwrap(), "no-store");
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
