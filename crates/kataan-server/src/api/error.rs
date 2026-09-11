//! The error type every handler returns, and the one place that decides which
//! `kataan_core::Error` is a 4xx.

use axum::{http::StatusCode, Json};
use tracing::{debug, error};

/// Map a core error to its HTTP status.
///
/// `InvalidRequest` is the caller's mistake — an unknown type, a malformed
/// timestamp, a bad filter, a field violating the type's schema — and must not
/// surface as a 500. One function, so reads and writes cannot come to disagree
/// about which variants are 4xx.
pub fn core_error(error: kataan_core::Error) -> ApiError {
    match error {
        kataan_core::Error::InvalidRequest(message) => ApiError::bad_request(message),
        // The id is not in the vault: the caller's mistake, and the same answer
        // `GET` gives. Blanket-mapping every NotFound I/O error would be wrong
        // — a missing config file is the server's problem — which is why core
        // reports this one as its own variant.
        kataan_core::Error::NotFound(message) => ApiError::not_found(message),
        // The request is well formed and would have been accepted a moment ago;
        // the caller needs to re-read, not correct its input.
        kataan_core::Error::Conflict(message) => ApiError::conflict(message),
        other => ApiError::from(other),
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    error: anyhow::Error,
}

impl ApiError {
    fn with_status(status: StatusCode, message: impl std::fmt::Display) -> Self {
        Self {
            status,
            error: anyhow::anyhow!("{message}"),
        }
    }

    pub fn not_found(message: impl std::fmt::Display) -> Self {
        Self::with_status(StatusCode::NOT_FOUND, message)
    }

    pub fn bad_request(message: impl std::fmt::Display) -> Self {
        Self::with_status(StatusCode::BAD_REQUEST, message)
    }

    pub fn conflict(message: impl std::fmt::Display) -> Self {
        Self::with_status(StatusCode::CONFLICT, message)
    }

    pub fn forbidden(message: impl std::fmt::Display) -> Self {
        Self::with_status(StatusCode::FORBIDDEN, message)
    }

    pub fn too_large(message: impl std::fmt::Display) -> Self {
        Self::with_status(StatusCode::PAYLOAD_TOO_LARGE, message)
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: error.into(),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        if self.status.is_server_error() {
            error!(status = %self.status, error = %self.error, "api request failed");
        } else {
            debug!(status = %self.status, error = %self.error, "api request rejected");
        }
        let body = Json(serde_json::json!({
            "ok": false,
            "error": self.error.to_string(),
        }));
        (self.status, body).into_response()
    }
}
