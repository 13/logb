//! QR sign-in: a one-time code a signed-in browser hands to a phone as a `logb://pair` URI (and
//! its QR rendering), redeemed for a token. Only the code's hash is ever stored -- see
//! `migrations/*/0017_pairing_codes.sql` / `0008_pairing_codes.sql` -- so this module owns
//! generation, hashing and the URI/QR the browser shows, while issuing and redeeming a code
//! (the database work) belongs to the API layer.

use base64ct::{Base64UrlUnpadded, Encoding as _};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rand::RngExt;
use sha2::{Digest, Sha256};
use std::time::Duration;

/// Random bytes behind one code. 32 bytes of a CSPRNG is far past brute-forceable, and gives a
/// 43-character base64url string with no padding to strip.
pub const CODE_BYTES: usize = 32;

/// How long an unredeemed code stays valid. Short, because it only has to survive the seconds
/// between showing the QR code and a phone scanning it.
pub const TTL: Duration = Duration::from_secs(5 * 60);

/// RFC 3986 unreserved characters left alone; everything else `NON_ALPHANUMERIC` would already
/// escape is escaped, including `:` and `/`. That is what carries a whole base URL as one query
/// value -- `encodeURIComponent`'s set, not a path- or fragment-specific one.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// A fresh, unpredictable code. Base64url without padding, so it is also safe to drop straight
/// into the `code` query parameter of `uri` without a second round of percent-encoding.
pub fn new_code() -> String {
    let mut bytes = [0u8; CODE_BYTES];
    rand::rng().fill(&mut bytes);
    Base64UrlUnpadded::encode_string(&bytes)
}

/// The value stored for a code: never the code itself, so a database leak does not hand out a
/// still-live sign-in. Hex, like every other stored hash in this codebase (`files::sha256_hex`).
pub fn hash(code: &str) -> String {
    hex::encode(Sha256::digest(code.as_bytes()))
}

/// The `logb://pair` deep link a QR code carries: `base_url` is this server's own address as
/// the phone must reach it, kept whole (trailing slash included) rather than trimmed, since the
/// app appends to it exactly as given.
pub fn uri(base_url: &str, code: &str) -> String {
    format!("logb://pair?server={}&code={code}", utf8_percent_encode(base_url, UNRESERVED))
}

/// The pairing URI as a scannable QR code, SVG text ready to inline into an HTML response.
///
/// `QrCode::new` only fails when the payload is too large for any QR version (about 2900 bytes
/// at the lowest error-correction level); a pairing URI is at most a few hundred, so the
/// `expect` never fires in practice.
pub fn qr_svg(uri: &str) -> String {
    qrcode::QrCode::new(uri)
        .expect("a pairing URI comfortably fits in a QR code")
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_43_url_safe_chars_and_differ_between_calls() {
        let a = new_code();
        let b = new_code();
        assert_eq!(a.len(), 43, "{a}");
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')), "{a}");
        assert_ne!(a, b);
    }

    #[test]
    fn hash_is_64_hex_chars_and_stable() {
        let code = new_code();
        let h = hash(&code);
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "{h}");
        assert_eq!(h, hash(&code));
    }

    #[test]
    fn hash_differs_for_different_codes() {
        assert_ne!(hash("code-one"), hash("code-two"));
    }

    #[test]
    fn uri_percent_encodes_the_base_and_keeps_the_trailing_slash() {
        assert_eq!(
            uri("https://logb.example/", "abc"),
            "logb://pair?server=https%3A%2F%2Flogb.example%2F&code=abc"
        );
    }

    #[test]
    fn qr_svg_is_svg_containing_the_uri_encoded_as_a_qr_code() {
        let svg = qr_svg(&uri("https://logb.example/", "abc"));
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"), "{svg}");
        assert!(svg.contains("</svg>"), "{svg}");
    }
}
