//! Server error types with axum integration.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use engram_core::api::ErrorResponse;

/// Application error that converts to HTTP responses.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Core(#[from] engram_core::Error),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

/// Generic message returned for all internal/unexpected errors.
/// Actual details are logged server-side but never sent to the client.
const INTERNAL_MSG: &str = "An internal error occurred";

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, "NOT_FOUND", msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", msg.clone()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", msg.clone()),
            AppError::Internal(msg) => {
                tracing::error!("Internal error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    INTERNAL_MSG.to_string(),
                )
            }
            AppError::Core(e) => {
                tracing::error!("Core error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    INTERNAL_MSG.to_string(),
                )
            }
            AppError::Anyhow(e) => {
                tracing::error!("Unexpected error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    INTERNAL_MSG.to_string(),
                )
            }
        };

        let body = ErrorResponse::new(message, code);
        (status, axum::Json(body)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
