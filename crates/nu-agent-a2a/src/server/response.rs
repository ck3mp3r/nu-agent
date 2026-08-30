use axum::{Json, response::IntoResponse};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// A2A response helpers (spec §11.4, §11.6)
// ---------------------------------------------------------------------------

/// Wrap a task/result value in the A2A response format.
pub fn a2a_ok(task: Value) -> Value {
    json!({ "task": task })
}

/// Build an A2A error response body (spec §11.6).
pub fn a2a_error(code: u16, status: &str, message: &str) -> Value {
    json!({
        "error": {
            "code": code,
            "status": status,
            "message": message,
            "details": [{
                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                "reason": status,
                "domain": "a2a-protocol.org",
            }],
        }
    })
}

/// Build an A2A error response with metadata.
pub fn a2a_error_with_meta(code: u16, status: &str, message: &str, metadata: Value) -> Value {
    json!({
        "error": {
            "code": code,
            "status": status,
            "message": message,
            "details": [{
                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                "reason": status,
                "domain": "a2a-protocol.org",
                "metadata": metadata,
            }],
        }
    })
}

pub fn a2a_json_response(body: Value) -> (axum::http::HeaderMap, Json<Value>) {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        // Static constant; from_static is infallible for a valid value.
        axum::http::HeaderValue::from_static("application/a2a+json"),
    );
    (headers, Json(body))
}

/// Small error wrapper for streaming handlers.
///
/// `axum::http::Response` is 128 bytes, which trips the `result_large_err`
/// lint when returned directly as a `Result` `Err`. Boxing it and implementing
/// `IntoResponse` keeps the error pointer-sized while satisfying both axum's
/// `Handler` bound and the lint.
#[derive(Debug)]
pub struct SseError(Box<axum::response::Response>);

impl SseError {
    pub fn new(response: axum::response::Response) -> Self {
        Self(Box::new(response))
    }
}

impl From<(axum::http::StatusCode, (axum::http::HeaderMap, Json<Value>))> for SseError {
    fn from(tuple: (axum::http::StatusCode, (axum::http::HeaderMap, Json<Value>))) -> Self {
        Self::new(tuple.into_response())
    }
}

impl axum::response::IntoResponse for SseError {
    fn into_response(self) -> axum::response::Response {
        *self.0
    }
}
