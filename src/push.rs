//! Browser push notifications: the identity this instance signs them with, and delivery of one
//! message to one subscribed browser.
//!
//! No third party is involved beyond the push service the browser itself chose (Google's for
//! Chrome, Mozilla's for Firefox, Apple's for Safari). The message is encrypted to the browser's
//! own key before it leaves, so that service carries it without being able to read it.

use crate::error::AppError;
use crate::state::App;
use base64ct::{Base64UrlUnpadded, Encoding as _};
use std::time::Duration;
use web_push_native::jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair};
use web_push_native::p256::PublicKey;
use web_push_native::{Auth, WebPushBuilder};

/// Key in `settings` holding this instance's VAPID private key, base64url.
const VAPID_KEY: &str = "vapid_private_key";
/// How long a push service may hold a message for a browser that is offline. A digest older
/// than a day has been replaced by the next one.
const TTL: Duration = Duration::from_secs(24 * 60 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// One browser's subscription, as `PushManager.subscribe` hands it out.
pub struct Subscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

/// How one delivery went.
#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    Sent,
    /// The push service says this subscription no longer exists (404 or 410): the browser
    /// unsubscribed, or the user cleared its data. The row should go.
    Gone,
    Failed(String),
}

/// The instance's VAPID key pair, generated and stored the first time anything asks.
///
/// Stored in the database rather than configured, so push works without the operator doing
/// anything, and every subscription keeps working across restarts and upgrades -- a browser
/// subscribes to one public key, and a different key would silently strand it. Two instances
/// racing to create it both insert with `ON CONFLICT DO NOTHING` and then read back whichever
/// won, so they agree.
pub async fn key_pair(state: &App) -> Result<ES256KeyPair, AppError> {
    if let Some(kp) = stored_key_pair(state).await? {
        return Ok(kp);
    }
    let fresh = Base64UrlUnpadded::encode_string(&ES256KeyPair::generate().to_bytes());
    sqlx::query("INSERT INTO settings (key, value) VALUES ($1, $2) ON CONFLICT (key) DO NOTHING")
        .bind(VAPID_KEY).bind(&fresh).execute(&state.db).await?;
    stored_key_pair(state).await?
        .ok_or_else(|| AppError::Internal("the VAPID key was not stored".into()))
}

async fn stored_key_pair(state: &App) -> Result<Option<ES256KeyPair>, AppError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = $1")
        .bind(VAPID_KEY).fetch_optional(&state.db).await?;
    let Some((encoded,)) = row else { return Ok(None) };
    let bytes = Base64UrlUnpadded::decode_vec(&encoded)
        .map_err(|_| AppError::Internal("the stored VAPID key is not base64url".into()))?;
    ES256KeyPair::from_bytes(&bytes)
        .map(Some)
        .map_err(|e| AppError::Internal(format!("the stored VAPID key is unusable: {e}")))
}

/// The public half, as `applicationServerKey` takes it: an uncompressed P-256 point, base64url.
pub fn public_key(kp: &ES256KeyPair) -> String {
    Base64UrlUnpadded::encode_string(&kp.public_key().public_key().to_bytes_uncompressed())
}

/// Who push services should contact about this sender. VAPID asks for a `mailto:` or `https:`
/// URI; the instance's public address is the one it has.
pub fn contact(state: &App) -> String {
    state.config.public_url.as_deref()
        .filter(|u| u.starts_with("https://"))
        .map(|u| u.trim_end_matches('/').to_string())
        .unwrap_or_else(|| "mailto:logb@localhost".to_string())
}

/// Refuses a subscription that could not be delivered to, or that points somewhere a browser's
/// push service never would.
///
/// Every real push endpoint is https. Accepting any URL would let a signed-in user make the
/// server POST to an arbitrary address -- inside the network it runs in, for instance -- once
/// a day. Plain http is allowed only to the loopback address, which is what the tests' stand-in
/// push service listens on.
pub fn validate(sub: &Subscription) -> Result<(), AppError> {
    let bad = |m: &str| AppError::BadRequest(m.to_string());
    let url = reqwest::Url::parse(&sub.endpoint).map_err(|_| bad("endpoint must be a URL"))?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return Err(bad("endpoint must be an https URL"));
    }
    let p256dh = Base64UrlUnpadded::decode_vec(&sub.p256dh).map_err(|_| bad("keys.p256dh must be base64url"))?;
    PublicKey::from_sec1_bytes(&p256dh).map_err(|_| bad("keys.p256dh is not a P-256 public key"))?;
    let auth = Base64UrlUnpadded::decode_vec(&sub.auth).map_err(|_| bad("keys.auth must be base64url"))?;
    if auth.len() != 16 {
        return Err(bad("keys.auth must be 16 bytes"));
    }
    Ok(())
}

/// Encrypts `payload` to one browser and hands it to its push service.
pub async fn send(kp: &ES256KeyPair, contact: &str, sub: &Subscription, payload: &[u8]) -> Delivery {
    match request(kp, contact, sub, payload) {
        Ok(req) => deliver(req).await,
        Err(e) => Delivery::Failed(e),
    }
}

fn request(kp: &ES256KeyPair, contact: &str, sub: &Subscription, payload: &[u8]) -> Result<axum::http::Request<Vec<u8>>, String> {
    let endpoint: axum::http::Uri = sub.endpoint.parse().map_err(|e| format!("endpoint: {e}"))?;
    let p256dh = Base64UrlUnpadded::decode_vec(&sub.p256dh).map_err(|e| format!("p256dh: {e}"))?;
    let auth = Base64UrlUnpadded::decode_vec(&sub.auth).map_err(|e| format!("auth: {e}"))?;
    let auth: [u8; 16] = auth.as_slice().try_into().map_err(|_| "auth must be 16 bytes".to_string())?;
    let key = PublicKey::from_sec1_bytes(&p256dh).map_err(|e| format!("p256dh: {e}"))?;
    WebPushBuilder::new(endpoint, key, Auth::from(auth))
        .with_valid_duration(TTL)
        .with_vapid(kp, contact)
        .build(payload.to_vec())
        .map_err(|e| format!("encrypt: {e}"))
}

async fn deliver(req: axum::http::Request<Vec<u8>>) -> Delivery {
    let client = match reqwest::Client::builder().timeout(HTTP_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => return Delivery::Failed(format!("client: {e}")),
    };
    let (parts, body) = req.into_parts();
    let sent = client.request(parts.method, parts.uri.to_string()).headers(parts.headers).body(body).send().await;
    match sent {
        Ok(res) if res.status().is_success() => Delivery::Sent,
        Ok(res) if matches!(res.status().as_u16(), 404 | 410) => Delivery::Gone,
        Ok(res) => Delivery::Failed(format!("push service returned {}", res.status())),
        // `without_url`: the endpoint is a capability for that browser, not something for a log.
        Err(e) => Delivery::Failed(format!("push post: {}", e.without_url())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P256DH: &str = "BLn9b-VR0ca83knDNZ32dCHGyjJp-1riX9ZTN40MqV8K_LpQmLqxC_DoHvqvFXO_nGdAB4W9dogZb_sM-uV4JbY";
    const AUTH: &str = "_ordMnz7uTCmrpBTeUV4Bw";

    fn sub(endpoint: &str) -> Subscription {
        Subscription { endpoint: endpoint.into(), p256dh: P256DH.into(), auth: AUTH.into() }
    }

    #[test]
    fn a_real_push_endpoint_is_accepted() {
        assert!(validate(&sub("https://fcm.googleapis.com/fcm/send/abc")).is_ok());
        assert!(validate(&sub("http://127.0.0.1:9999/push")).is_ok(), "loopback, for tests");
    }

    #[test]
    fn anything_a_browser_would_never_hand_out_is_refused() {
        for endpoint in ["http://192.168.1.1/admin", "http://router.local/", "ftp://example.com/", "not a url"] {
            assert!(validate(&sub(endpoint)).is_err(), "{endpoint}");
        }
        assert!(validate(&Subscription { auth: "AAAA".into(), ..sub("https://push.example/") }).is_err());
        assert!(validate(&Subscription { p256dh: "AAAA".into(), ..sub("https://push.example/") }).is_err());
    }

    #[test]
    fn a_request_is_encrypted_and_signed() {
        let kp = ES256KeyPair::generate();
        let req = request(&kp, "mailto:a@b.c", &sub("https://push.example/x"), b"hello").unwrap();
        assert_eq!(req.headers()["content-encoding"], "aes128gcm");
        assert!(req.headers()["authorization"].to_str().unwrap().starts_with("vapid t="));
        assert_ne!(req.body().as_slice(), b"hello");
    }
}
