//! Shared HTTP error mapping for all route handlers.

use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;

/// Catch-all error type turning coach-core errors into HTTP status codes.
#[derive(Debug)]
pub struct ApiError(pub coach_core::CoreError);

impl From<coach_core::CoreError> for ApiError {
    fn from(e: coach_core::CoreError) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0 {
            coach_core::CoreError::NotFound(_) => StatusCode::NOT_FOUND,
            coach_core::CoreError::Conflict(_) => StatusCode::CONFLICT,
            coach_core::CoreError::InvalidQuery(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({"error": self.0.to_string()}))).into_response()
    }
}
