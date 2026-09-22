use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    /// A 400 with a stable `error` code more specific than `bad_request`, so a client can say it
    /// in the reader's language and fall back to `message` for a code it does not know.
    #[error("{message}")]
    Invalid { code: &'static str, message: String },
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

/// Whether this is the database saying "not now" rather than "no". Both mean the request never
/// ran and sending it again is the right thing to do.
///
/// `PoolTimedOut` is a writer that waited out `db::WRITE_WAIT` for the one writer connection.
/// SQLite codes 5 and 6 are `SQLITE_BUSY` and `SQLITE_LOCKED`, which a single-statement write on
/// the read pool can still meet while something long-running holds the lock -- an import, a
/// database copy, the retention purge.
fn is_write_contention(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::PoolTimedOut => true,
        sqlx::Error::Database(d) => matches!(d.code().as_deref(), Some("5") | Some("6")),
        _ => false,
    }
}

impl AppError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Invalid { code, .. } => (StatusCode::BAD_REQUEST, code),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::InUse(_) => (StatusCode::CONFLICT, "in_use"),
            AppError::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            AppError::Gone => (StatusCode::GONE, "gone"),
            AppError::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
            AppError::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            AppError::Db(e) if is_write_contention(e) => {
                (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
            }
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
        let busy = matches!(&self, AppError::Db(e) if is_write_contention(e));
        let message = if busy {
            // Warn, not error: nothing is broken, and the client is being asked to come back.
            //
            // The two halves are logged apart on purpose. A database that said "busy" is one
            // writer losing a race it will win next time. A pool timeout means the single writer
            // connection was not handed back within `db::WRITE_WAIT`, which is a long import or
            // copy -- or a transaction nobody closed, the one bug this answer could otherwise
            // hide behind a retry forever. Searching for the second line finds it.
            if matches!(&self, AppError::Db(sqlx::Error::PoolTimedOut)) {
                tracing::warn!(
                    "waited {:?} for the writer connection and did not get it; asked the client to retry",
                    crate::db::WRITE_WAIT
                );
            } else {
                tracing::warn!(error = %self, "the database was busy; asked the client to retry");
            }
            "the database is busy, please retry".to_string()
        } else if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "request failed");
            "internal error".to_string()
        } else {
            self.to_string()
        };
        let body = match self {
            AppError::InUse(count) => json!({ "error": code, "message": message, "count": count }),
            _ => json!({ "error": code, "message": message }),
        };
        if busy {
            // One second is the smallest value that means "not immediately"; a client that
            // queues writes (the bundled one does) uses its own backoff from here.
            return (status, [(axum::http::header::RETRY_AFTER, "1")], Json(body)).into_response();
        }
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
