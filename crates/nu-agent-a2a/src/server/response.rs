use axum::Json;
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
        "application/a2a+json".parse().expect("static content-type"),
    );
    (headers, Json(body))
}
