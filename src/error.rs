use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("authentication required")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("this cursor cannot be resumed -- re-bootstrap")]
    Gone,
    #[error("payload too large")]
    TooLarge,
    #[error("too many requests")]
    TooManyRequests,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Internal(String),
    /// A delete refused because `count` rows still use the thing. The count travels in the body
    /// so a client can say "used by 2 objects" without a second request.
    #[error("still used by {0} object(s)")]
    InUse(i64),
}

impl AppError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::InUse(_) => (StatusCode::CONFLICT, "in_use"),
            AppError::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            AppError::Gone => (StatusCode::GONE, "gone"),
            AppError::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
            AppError::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            AppError::Db(sqlx::Error::RowNotFound) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Db(_) | AppError::Io(_) | AppError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "request failed");
            "internal error".to_string()
        } else {
            self.to_string()
        };
        let body = match self {
            AppError::InUse(count) => json!({ "error": code, "message": message, "count": count }),
            _ => json!({ "error": code, "message": message }),
        };
        (status, Json(body)).into_response()
    }
}

/// Lets handler code turn a manual `Json` extraction into an `AppError`.
/// (Extractor rejections in a handler signature render themselves; this is for explicit conversions.)
impl From<axum::extract::rejection::JsonRejection> for AppError {
    fn from(r: axum::extract::rejection::JsonRejection) -> Self {
        AppError::BadRequest(r.body_text())
    }
}
