use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rust_embed::Embed;
use serde_json::json;

#[derive(Embed)]
#[folder = "frontend/dist/"]
struct Assets;

pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path == "api" {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found", "message": "no such route" }))).into_response();
    }
    let (name, cache) = if !path.is_empty() && Assets::get(path).is_some() {
        let immutable = path.starts_with("assets/");
        (path.to_string(), if immutable { "public, max-age=31536000, immutable" } else { "no-cache" })
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
