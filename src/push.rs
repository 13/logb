//! Browser push notifications: the identity this instance signs them with, and delivery of one
//! message to one subscribed browser.
//!
//! No third party is involved beyond the push service the browser itself chose (Google's for
//! Chrome, Mozilla's for Firefox, Apple's for Safari). The message is encrypted to the browser's
//! own key before it leaves, so that service carries it without being able to read it.

use crate::error::AppError;
use crate::state::App;
use base64ct::{Base64UrlUnpadded, Encoding as _};
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey};
use rand::RngExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use web_push_native::p256::PublicKey;
use web_push_native::{Auth, WebPushBuilder};

/// Key in `settings` holding this instance's VAPID private key: the raw 32-byte P-256 secret
/// scalar, base64url.
const VAPID_KEY: &str = "vapid_private_key";
/// How long a push service may hold a message for a browser that is offline. A digest older
/// than a day has been replaced by the next one.
const TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// How long one VAPID token is good for. RFC 8292 lets push services refuse an `exp` more than
/// 24 hours ahead; a token at exactly 24 hours sits on that limit, and a push service whose
/// clock lags ours by a second would refuse it. Half that is well clear, and every send signs
/// a fresh token anyway.
const VAPID_VALIDITY: Duration = Duration::from_secs(12 * 60 * 60);
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
pub async fn key_pair(state: &App) -> Result<SigningKey, AppError> {
    if let Some(kp) = stored_key_pair(state).await? {
        return Ok(kp);
    }
    let fresh = encode_key(&generate_key());
    sqlx::query("INSERT INTO settings (key, value) VALUES ($1, $2) ON CONFLICT (key) DO NOTHING")
        .bind(VAPID_KEY)
        .bind(&fresh)
        .execute(&state.db)
        .await?;
    stored_key_pair(state)
        .await?
        .ok_or_else(|| AppError::Internal("the VAPID key was not stored".into()))
}

async fn stored_key_pair(state: &App) -> Result<Option<SigningKey>, AppError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = $1")
        .bind(VAPID_KEY)
        .fetch_optional(&state.db)
        .await?;
    let Some((encoded,)) = row else {
        return Ok(None);
    };
    decode_key(&encoded).map(Some).map_err(AppError::Internal)
}

/// Reads the stored form. It is the one earlier releases wrote (jwt-simple's
/// `ES256KeyPair::to_bytes`, the bare secret scalar), so an existing instance keeps its key.
fn decode_key(encoded: &str) -> Result<SigningKey, String> {
    let bytes = Base64UrlUnpadded::decode_vec(encoded)
        .map_err(|_| "the stored VAPID key is not base64url".to_string())?;
    SigningKey::from_slice(&bytes).map_err(|e| format!("the stored VAPID key is unusable: {e}"))
}

fn encode_key(kp: &SigningKey) -> String {
    Base64UrlUnpadded::encode_string(&kp.to_bytes())
}

/// A new random key.
///
/// `p256` wants an RNG from `rand_core` 0.6, and this project's `rand` is built on a later one,
/// so rather than bridge the two it draws 32 bytes the way the rest of the app draws randomness
/// and uses them as the secret scalar. A draw that is zero or not below the curve order is no
/// key; that happens about once in 2^32 draws, and drawing again is all it takes.
fn generate_key() -> SigningKey {
    loop {
        let mut bytes = [0u8; 32];
        rand::rng().fill(&mut bytes);
        if let Ok(kp) = SigningKey::from_bytes(&bytes.into()) {
            return kp;
        }
    }
}

/// The public half, as `applicationServerKey` takes it: an uncompressed P-256 point, base64url.
pub fn public_key(kp: &SigningKey) -> String {
    Base64UrlUnpadded::encode_string(kp.verifying_key().to_encoded_point(false).as_bytes())
}

/// Who push services should contact about this sender. VAPID asks for a `mailto:` or `https:`
/// URI; the instance's public address is the one it has.
pub fn contact(state: &App) -> String {
    state
        .config
        .public_url
        .as_deref()
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
    let p256dh = Base64UrlUnpadded::decode_vec(&sub.p256dh)
        .map_err(|_| bad("keys.p256dh must be base64url"))?;
    PublicKey::from_sec1_bytes(&p256dh)
        .map_err(|_| bad("keys.p256dh is not a P-256 public key"))?;
    let auth =
        Base64UrlUnpadded::decode_vec(&sub.auth).map_err(|_| bad("keys.auth must be base64url"))?;
    if auth.len() != 16 {
        return Err(bad("keys.auth must be 16 bytes"));
    }
    Ok(())
}

/// Encrypts `payload` to one browser and hands it to its push service.
pub async fn send(kp: &SigningKey, contact: &str, sub: &Subscription, payload: &[u8]) -> Delivery {
    match request(kp, contact, sub, payload) {
        Ok(req) => deliver(req).await,
        Err(e) => Delivery::Failed(e),
    }
}

fn request(
    kp: &SigningKey,
    contact: &str,
    sub: &Subscription,
    payload: &[u8],
) -> Result<axum::http::Request<Vec<u8>>, String> {
    let endpoint: axum::http::Uri = sub.endpoint.parse().map_err(|e| format!("endpoint: {e}"))?;
    let p256dh = Base64UrlUnpadded::decode_vec(&sub.p256dh).map_err(|e| format!("p256dh: {e}"))?;
    let auth = Base64UrlUnpadded::decode_vec(&sub.auth).map_err(|e| format!("auth: {e}"))?;
    let auth: [u8; 16] = auth
        .as_slice()
        .try_into()
        .map_err(|_| "auth must be 16 bytes".to_string())?;
    let key = PublicKey::from_sec1_bytes(&p256dh).map_err(|e| format!("p256dh: {e}"))?;
    let authorization = vapid_authorization(kp, contact, &endpoint)?;
    let mut req = WebPushBuilder::new(endpoint, key, Auth::from(auth))
        .with_valid_duration(TTL)
        .build(payload.to_vec())
        .map_err(|e| format!("encrypt: {e}"))?;
    let authorization = axum::http::HeaderValue::try_from(authorization)
        .map_err(|e| format!("authorization: {e}"))?;
    req.headers_mut()
        .insert(axum::http::header::AUTHORIZATION, authorization);
    Ok(req)
}

/// The `Authorization` header RFC 8292 asks for: `vapid t=<JWT>, k=<public key>`.
///
/// Built here rather than by `web-push-native`, whose VAPID support comes through `jwt-simple`
/// and with it an RSA implementation this instance never uses and `cargo audit` flags. The token
/// is only ever ES256, so it is a fixed header, three claims and one P-256 signature.
fn vapid_authorization(
    kp: &SigningKey,
    contact: &str,
    endpoint: &axum::http::Uri,
) -> Result<String, String> {
    // Scheme and host only, as the audience has always been sent from here: every push service's
    // endpoint uses the default port, so this is the origin they check it against.
    let scheme = endpoint.scheme_str().ok_or("endpoint has no scheme")?;
    let host = endpoint.host().ok_or("endpoint has no host")?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("clock: {e}"))?;
    let claims = serde_json::json!({
        "aud": format!("{scheme}://{host}"),
        "sub": contact,
        "exp": (now + VAPID_VALIDITY).as_secs(),
    });
    let header = Base64UrlUnpadded::encode_string(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = Base64UrlUnpadded::encode_string(claims.to_string().as_bytes());
    let signing_input = format!("{header}.{claims}");
    // ECDSA over SHA-256; JWS wants the signature as the fixed 64-byte `r || s`, not DER.
    let signature: Signature = kp.sign(signing_input.as_bytes());
    let signature = Base64UrlUnpadded::encode_string(&signature.to_bytes());
    Ok(format!(
        "vapid t={signing_input}.{signature}, k={}",
        public_key(kp)
    ))
}

async fn deliver(req: axum::http::Request<Vec<u8>>) -> Delivery {
    let client = match reqwest::Client::builder().timeout(HTTP_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => return Delivery::Failed(format!("client: {e}")),
    };
    let (parts, body) = req.into_parts();
    let sent = client
        .request(parts.method, parts.uri.to_string())
        .headers(parts.headers)
        .body(body)
        .send()
        .await;
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

    const P256DH: &str =
        "BLn9b-VR0ca83knDNZ32dCHGyjJp-1riX9ZTN40MqV8K_LpQmLqxC_DoHvqvFXO_nGdAB4W9dogZb_sM-uV4JbY";
    const AUTH: &str = "_ordMnz7uTCmrpBTeUV4Bw";

    fn sub(endpoint: &str) -> Subscription {
        Subscription {
            endpoint: endpoint.into(),
            p256dh: P256DH.into(),
            auth: AUTH.into(),
        }
    }

    #[test]
    fn a_real_push_endpoint_is_accepted() {
        assert!(validate(&sub("https://fcm.googleapis.com/fcm/send/abc")).is_ok());
        assert!(
            validate(&sub("http://127.0.0.1:9999/push")).is_ok(),
            "loopback, for tests"
        );
    }

    #[test]
    fn anything_a_browser_would_never_hand_out_is_refused() {
        for endpoint in [
            "http://192.168.1.1/admin",
            "http://router.local/",
            "ftp://example.com/",
            "not a url",
        ] {
            assert!(validate(&sub(endpoint)).is_err(), "{endpoint}");
        }
        assert!(validate(&Subscription {
            auth: "AAAA".into(),
            ..sub("https://push.example/")
        })
        .is_err());
        assert!(validate(&Subscription {
            p256dh: "AAAA".into(),
            ..sub("https://push.example/")
        })
        .is_err());
    }

    /// A secret as the database holds it (bytes 0x01..=0x20) and the public key the previous,
    /// jwt-simple based code derived from it. Browsers already subscribed to this instance's key.
    const STORED_SECRET: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA";
    const STORED_PUBLIC: &str =
        "BFFcPW6545a5BNP-yn9U_c0MwemXvzddylFa0KbDtANfRTa-OlDzGPv5pUdZAqIhUCvvDVfgjFOyzApW8X2fk1Q";

    #[test]
    fn the_stored_key_format_is_unchanged() {
        let kp = decode_key(STORED_SECRET).unwrap();
        assert_eq!(public_key(&kp), STORED_PUBLIC);
        assert_eq!(encode_key(&kp), STORED_SECRET);

        let fresh = generate_key();
        let stored = encode_key(&fresh);
        assert_eq!(Base64UrlUnpadded::decode_vec(&stored).unwrap().len(), 32);
        assert_eq!(
            public_key(&decode_key(&stored).unwrap()),
            public_key(&fresh)
        );
    }

    #[test]
    fn the_vapid_header_is_a_valid_es256_jwt() {
        use p256::ecdsa::signature::Verifier as _;
        use p256::ecdsa::{Signature, VerifyingKey};

        let kp = generate_key();
        let req = request(
            &kp,
            "mailto:a@b.c",
            &sub("https://push.example:8443/x"),
            b"hello",
        )
        .unwrap();
        let header = req.headers()["authorization"].to_str().unwrap();
        let rest = header.strip_prefix("vapid t=").expect(header);
        let (jwt, k) = rest.split_once(", k=").expect(header);
        assert_eq!(k, public_key(&kp));

        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "{jwt}");
        let json = |part: &str| -> serde_json::Value {
            serde_json::from_slice(&Base64UrlUnpadded::decode_vec(part).unwrap()).unwrap()
        };
        assert_eq!(
            json(parts[0]),
            serde_json::json!({"typ": "JWT", "alg": "ES256"})
        );
        let claims = json(parts[1]);
        assert_eq!(claims["aud"], "https://push.example");
        assert_eq!(claims["sub"], "mailto:a@b.c");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let exp = claims["exp"].as_u64().expect("exp is an integer");
        let expected = now + 12 * 60 * 60;
        assert!(
            (expected - 60..=expected + 60).contains(&exp),
            "exp {exp}, expected about {expected}"
        );

        let verifying =
            VerifyingKey::from_sec1_bytes(&Base64UrlUnpadded::decode_vec(k).unwrap()).unwrap();
        let signature =
            Signature::from_slice(&Base64UrlUnpadded::decode_vec(parts[2]).unwrap()).unwrap();
        verifying
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .unwrap();
    }

    #[test]
    fn a_request_is_encrypted_and_signed() {
        let kp = generate_key();
        let req = request(
            &kp,
            "mailto:a@b.c",
            &sub("https://push.example/x"),
            b"hello",
        )
        .unwrap();
        assert_eq!(req.headers()["content-encoding"], "aes128gcm");
        assert!(req.headers()["authorization"]
            .to_str()
            .unwrap()
            .starts_with("vapid t="));
        assert_ne!(req.body().as_slice(), b"hello");
    }
}
