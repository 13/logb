//! QR sign-in's two endpoints: a signed-in browser asks for a one-time code
//! (`POST /auth/pair`), a phone swaps it for an API token (`POST /auth/pair/redeem`). The code
//! itself -- generation, hashing, the URI and its QR rendering -- lives in `domain::pairing`;
//! this module owns the database work and the HTTP shapes around it.

use crate::auth::{self, SessionUser};
use crate::db;
use crate::domain::pairing;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::str::FromStr;

pub fn router() -> Router<App> {
    Router::new()
        .route("/auth/pair", post(create_pair))
        .route("/auth/pair/redeem", post(redeem))
}

/// The origin this instance is reachable at, to embed in a pairing URI's `server` parameter.
///
/// `LOGB_PUBLIC_URL` (`state.config.public_url`) is authoritative when set: it already exists
/// for exactly this question -- "where do I tell someone this instance lives" -- and `notify`
/// uses it the same way to put links into the reminder digest. Reusing it here means an
/// operator only states their public address once.
///
/// Unset, the origin is rebuilt from the request the browser making this call actually used:
/// the scheme from `auth::wants_secure` -- the same signal that decides the session cookie's
/// `Secure` flag -- and the host from the `Host` header. `X-Forwarded-Host` is read instead
/// only when `LOGB_TRUST_PROXY` says this deployment sits behind a proxy that overwrites it --
/// the same trust boundary `auth::client_ip` already applies to `X-Forwarded-For` -- so a
/// caller cannot hand a signed-in browser's own QR code an arbitrary host by forging a header
/// on a deployment that never promised to rewrite one.
///
/// A proxy is free to append to `X-Forwarded-Host` rather than replace it, so -- exactly as
/// `auth::client_ip` does for `X-Forwarded-For` -- only the first comma-separated entry is
/// read, trimmed. Whichever header ends up supplying the host, its value is checked against
/// `http::uri::Authority` syntax before it is glued into a URI this browser will render as a
/// link and a QR code: a value that is not valid `host[:port]` (an embedded `/` or `?` that
/// would smuggle extra path or query into the pairing URI, say) is a 400 rather than a broken
/// or hostile deep link.
fn public_base_url(state: &App, headers: &HeaderMap) -> Result<String, AppError> {
    if let Some(url) = state.config.public_url.as_deref() {
        let trimmed = url.trim_end_matches('/');
        return Ok(format!("{trimmed}/"));
    }
    let scheme = if auth::wants_secure(state, headers) { "https" } else { "http" };
    let forwarded_host = state
        .config
        .trust_proxy
        .then(|| headers.get("x-forwarded-host"))
        .flatten()
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').next().unwrap_or("").trim())
        .filter(|v| !v.is_empty());
    let host_header =
        headers.get(axum::http::header::HOST).and_then(|v| v.to_str().ok()).map(str::trim);
    for candidate in [forwarded_host, host_header].into_iter().flatten() {
        if axum::http::uri::Authority::from_str(candidate).is_err() {
            return Err(AppError::BadRequest(format!("invalid host {candidate:?}")));
        }
    }
    let host = forwarded_host.or(host_header).unwrap_or("localhost");
    Ok(format!("{scheme}://{host}/"))
}

/// Refuses a same-site check ONLY when the browser's `Sec-Fetch-Site` header actively says the
/// request crossed a site boundary -- present, and neither `same-origin` nor `none`.
///
/// `create_pair` takes a session cookie and no JSON body, so it is exactly the shape a plain
/// cross-site `<form method=post>` can hit: the cookie rides along automatically, and a form
/// needs no CORS preflight to fire, unlike a cross-site `fetch()` sending a body a form cannot
/// send. `create_token` needs no equivalent check despite taking the same `SessionUser`,
/// because it requires `Content-Type: application/json` -- a plain HTML form cannot set that
/// content type, so the cross-site form vector this function closes for `create_pair` does not
/// exist against `create_token` in the first place.
///
/// `none` is allowed alongside `same-origin` rather than treated as "no header, so unknown":
/// browsers send it for a top-level navigation the user typed or clicked into existence
/// themselves (the address bar, a bookmark, a search result) rather than for any subresource
/// request an already-loaded page issues on its own -- `fetch()`, which is all this route is
/// ever called from, cannot produce it. Allowing it here is therefore not a hole a same-site
/// form could climb through; it only accommodates a browser old enough to send the header
/// inconsistently, or a future first-party client this route has not been asked to distrust.
/// A missing header (an old browser, or a non-browser API client) is let through for the same
/// reason `create_token` needs no check at all: the absence of Fetch Metadata is not evidence
/// of a cross-site form, only of a browser that predates it.
fn same_site_request(headers: &HeaderMap) -> Result<(), AppError> {
    match headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        None | Some("same-origin") | Some("none") => Ok(()),
        Some(_) => Err(AppError::Forbidden),
    }
}

/// Issues a fresh pairing code for the caller: a browser's Account page, not the phone.
///
/// `SessionUser`, exactly like `create_token` -- creating something that can sign a device in
/// needs an interactive login, never a token that could mint another credential on its own.
async fn create_pair(
    SessionUser(user): SessionUser,
    State(state): State<App>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    // See `same_site_request`'s own comment for why this route needs it and `create_token`
    // does not: no JSON body here means no content-type guard against a plain cross-site form.
    same_site_request(&headers)?;

    // Resolved -- and, on an unset LOGB_PUBLIC_URL, validated -- before anything is written: a
    // malformed X-Forwarded-Host or Host must fail this request without first creating and
    // committing a code the caller never gets back in a usable response.
    let base_url = public_base_url(&state, &headers)?;

    let now = db::now();
    let code = pairing::new_code();
    let expires_at = (chrono::Utc::now() + chrono::Duration::from_std(pairing::TTL).unwrap())
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // A new code replaces whatever this user already had -- used, expired, or still perfectly
    // live -- not just the dead rows: the frontend shows only ever the most recent code, so an
    // older one that could still be redeemed would be a working sign-in nobody can see any more
    // to notice or revoke. Delete and insert share one write transaction (the same
    // `db::begin_write` `redeem` uses) so a crash between them cannot leave this user with the
    // old code deleted and no new one in its place.
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("DELETE FROM pairing_codes WHERE user_id = $1")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO pairing_codes (user_id, code_hash, created_at, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(user.id)
        .bind(pairing::hash(&code))
        .bind(&now)
        .bind(&expires_at)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let uri = pairing::uri(&base_url, &code);
    let qr_svg = pairing::qr_svg(&uri);
    Ok((StatusCode::CREATED, Json(json!({
        "code": code,
        "uri": uri,
        "qr_svg": qr_svg,
        "expires_at": expires_at,
    }))))
}

#[derive(Deserialize)]
struct PairRedeem {
    code: String,
    device_name: String,
}

/// Unicode "Format" (Cf) code points a device name has no business carrying: characters with no
/// glyph of their own whose entire job is to change how neighbouring characters are laid out or
/// read. `char::is_control` (category Cc) does not cover this category at all -- it is a
/// different one -- so a bidi override such as U+202E, which can make the rest of a token name
/// display as if it read right to left, sails straight through it. This list is not every Cf
/// code point Unicode has ever assigned (a handful of format controls for historic scripts are
/// left out), but it covers the ones an ordinary keyboard, IME or copy-paste can produce: the
/// zero-width and bidi-control block, the deprecated Arabic number-shape signs, the interlinear
/// annotation controls, and the language/emoji tag range.
fn is_unicode_format_char(c: char) -> bool {
    matches!(c,
        '\u{00AD}' // soft hyphen
        | '\u{0600}'..='\u{0605}' // Arabic number-shape / sign signs
        | '\u{061C}' // Arabic letter mark
        | '\u{06DD}'
        | '\u{070F}' // Syriac abbreviation mark
        | '\u{08E2}'
        | '\u{180E}' // Mongolian vowel separator
        | '\u{200B}'..='\u{200F}' // ZWSP, ZWNJ, ZWJ, LRM, RLM
        | '\u{202A}'..='\u{202E}' // LRE, RLE, PDF, LRO, RLO -- the bidi overrides
        | '\u{2060}'..='\u{2064}' // word joiner, invisible operators
        | '\u{2066}'..='\u{206F}' // bidi isolates, and a few deprecated format characters
        | '\u{FEFF}' // BOM / zero width no-break space
        | '\u{FFF9}'..='\u{FFFB}' // interlinear annotation anchor/separator/terminator
        | '\u{E0001}' // language tag
        | '\u{E0020}'..='\u{E007F}' // tag characters
    )
}

/// A device name a person did not choose, cleaned to plain, visible text before it becomes part
/// of an API token's name: control characters (category Cc) and Unicode format characters
/// (category Cf, see `is_unicode_format_char`) are dropped outright rather than merely trimmed,
/// runs of whitespace collapse to a single space, and the ends are trimmed. `split_whitespace`
/// does both the collapsing and the trimming in one pass.
///
/// A name that is empty, or was nothing BUT whitespace and/or the characters just stripped, is
/// refused with a 400 rather than silently accepted as an empty string glued onto the prefix --
/// the same judgment `create_token` already makes about a name a person types by hand.
fn clean_device_name(device_name: &str) -> Result<String, AppError> {
    let visible: String =
        device_name.chars().filter(|c| !c.is_control() && !is_unicode_format_char(*c)).collect();
    let cleaned = visible.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return Err(AppError::BadRequest("device_name must not be empty".into()));
    }
    Ok(cleaned)
}

/// `LogB Android · <device name>`, cleaned (see `clean_device_name`), truncated by character to
/// 64 -- the same length `create_token` enforces on a name a person types, applied here to one a
/// device supplies instead -- and trimmed again, since truncating by character count can land
/// exactly on a space the cleaning step left inside the name.
fn device_token_name(device_name: &str) -> Result<String, AppError> {
    let cleaned = clean_device_name(device_name)?;
    let full = format!("LogB Android · {cleaned}");
    let truncated: String = full.chars().take(64).collect();
    Ok(truncated.trim().to_string())
}

/// Swaps a one-time code for a named API token. No session or token of its own is needed -- the
/// code itself is the credential -- so this is rate-limited exactly like `login`, by the
/// calling IP.
///
/// Unknown, expired and already-used codes are told apart from nothing: the `UPDATE ...
/// RETURNING` below only ever matches a code that is both unused and unexpired, so every other
/// case reaches the same `AppError::Unauthorized` and therefore the same response body. A
/// caller probing codes learns nothing about which of the three it tried.
async fn redeem(
    State(state): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PairRedeem>,
) -> Result<Json<serde_json::Value>, AppError> {
    auth::check_login_rate(&state, auth::client_ip(&state, &headers, peer))?;
    // Validated before the code is touched: a device name that fails this must not burn a
    // one-time code for nothing -- the phone can fix the name and try the same code again.
    let name = device_token_name(&body.device_name)?;

    let now = db::now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let redeemed: Option<(i64,)> = sqlx::query_as(
        "UPDATE pairing_codes SET used_at = $1 WHERE code_hash = $2 AND used_at IS NULL AND expires_at > $3 \
         RETURNING user_id",
    )
    .bind(&now)
    .bind(pairing::hash(&body.code))
    .bind(&now)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_id,)) = redeemed else {
        return Err(AppError::Unauthorized);
    };

    // Minted in the same transaction that marked the code used: a crash between the two would
    // otherwise either burn a valid code for no token, or hand out a token from a redeem that
    // never committed.
    let token = auth::new_api_token();
    let id: (i64,) = sqlx::query_as(
        "INSERT INTO api_tokens (user_id, name, token_hash, prefix, created_at) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(user_id)
    .bind(&name)
    .bind(auth::hash_api_token(&token))
    .bind(auth::token_prefix(&token))
    .bind(&now)
    .fetch_one(&mut *tx)
    .await?;
    let user: (i64, String) = sqlx::query_as("SELECT id, username FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(json!({
        "token": token,
        "token_id": id.0,
        "user": { "id": user.0, "username": user.1 },
    })))
}
