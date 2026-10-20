//! API errors: `{"error": "..."}` with a status, as both processes answer
//! them.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// An API error: `{"error": "..."}` with a status, and a `reason` code
/// where a client acts on which refusal it was (#327).
#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String, pub Option<&'static str>);

pub fn err(status: StatusCode, msg: &str) -> ApiError {
    ApiError(status, msg.to_owned(), None)
}

/// An error with a reason code beside the sentence.
pub fn refusal(status: StatusCode, reason: &'static str, msg: &str) -> ApiError {
    ApiError(status, msg.to_owned(), Some(reason))
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::warn!(error = %e, "internal error");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong".into(), None)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        anyhow::Error::from(e).into()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self.2 {
            Some(reason) => (self.0, Json(json!({ "error": self.1, "reason": reason }))).into_response(),
            None => (self.0, Json(json!({ "error": self.1 }))).into_response(),
        }
    }
}

/// A typed answer, as JSON.
pub fn reply<T: serde::Serialize>(answer: &T) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(serde_json::to_value(answer).map_err(anyhow::Error::from)?))
}
