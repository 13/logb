use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rust_embed::Embed;
use serde_json::json;

#[derive(Embed)]
#[folder = "frontend/dist/"]
struct Assets;

/// The result of canonicalising a request path.
struct NormalizedPath {
    /// True if the first normalised segment is exactly `api` (covers `/api`, `/api/`,
    /// `/api/x`, `/./api/x` and `//api/x`, while leaving `/apifoo` to the frontend).
    is_api: bool,
    /// The path to use for the embedded-asset lookup: `/`-joined, with empty segments
    /// (collapsing `//`), `.` segments, and `..` segments all dropped. Dropping rather than
    /// resolving `..` means this string can never contain `..`, so it can never be read as
    /// walking anywhere - though since `Assets::get` is a compile-time map lookup rather
    /// than a filesystem read, there is no real directory to walk out of; this is purely
    /// about keeping the `/api` guarantee correct for non-canonical paths.
    asset_path: String,
}

/// Split a URI path into normalised segments and classify it, so the `/api` check and the
/// asset lookup both operate on the same canonical view of the path.
fn normalize(path: &str) -> NormalizedPath {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty() && *s != "." && *s != "..").collect();
    let is_api = segments.first() == Some(&"api");
    NormalizedPath { is_api, asset_path: segments.join("/") }
}

pub async fn handler(uri: Uri) -> Response {
    let normalized = normalize(uri.path());
    if normalized.is_api {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found", "message": "no such route" }))).into_response();
    }
    let path = normalized.asset_path;
    let (name, cache) = if !path.is_empty() && Assets::get(&path).is_some() {
        let immutable = path.starts_with("assets/");
        (path, if immutable { "public, max-age=31536000, immutable" } else { "no-cache" })
    } else {
        ("index.html".to_string(), "no-cache")
    };
    match Assets::get(&name) {
        Some(file) => {
            let mime = mime_guess::from_path(&name).first_or_octet_stream();
            (
                [
                    (header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap()),
                    (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
                ],
                file.data.into_owned(),
            ).into_response()
        }
        None => (StatusCode::SERVICE_UNAVAILABLE, "frontend not built: run `npm run build` in frontend/").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_api_and_trailing_slash_are_api() {
        assert!(normalize("/api").is_api);
        assert!(normalize("/api/").is_api);
    }

    #[test]
    fn nested_api_route_is_api() {
        assert!(normalize("/api/does-not-exist").is_api);
    }

    #[test]
    fn dot_segment_prefix_is_still_api() {
        // Bypasses the old `starts_with("api/")` check: trimmed path is `./api/does-not-exist`.
        assert!(normalize("/./api/does-not-exist").is_api);
    }

    #[test]
    fn double_slash_prefix_is_still_api() {
        // Axum's matcher requires a literal `/api` prefix, so this falls through to us too.
        assert!(normalize("//api/does-not-exist").is_api);
    }

    #[test]
    fn dot_dot_segment_under_api_is_still_api() {
        assert!(normalize("/api/../does-not-exist").is_api);
    }

    #[test]
    fn apifoo_is_not_api() {
        assert!(!normalize("/apifoo").is_api);
    }

    #[test]
    fn dot_dot_outside_api_is_not_api() {
        assert!(!normalize("/../api-like/x").is_api);
    }

    #[test]
    fn asset_path_never_contains_dot_dot() {
        let n = normalize("/../../etc/passwd");
        assert!(!n.asset_path.contains(".."));
        assert_eq!(n.asset_path, "etc/passwd");
    }

    #[test]
    fn asset_path_collapses_double_slashes_and_dot_segments() {
        assert_eq!(normalize("//assets//app.js").asset_path, "assets/app.js");
        assert_eq!(normalize("/./assets/./app.js").asset_path, "assets/app.js");
    }
}
