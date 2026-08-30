use axum::{Json, http::StatusCode, middleware::Next, response::IntoResponse};

use super::response::a2a_error;

// ---------------------------------------------------------------------------
// A2A-Version middleware (A2A spec §9.2, §14.2)
// ---------------------------------------------------------------------------

/// Axum middleware that validates the incoming `A2A-Version` header on A2A API
/// paths and adds the `A2A-Version` header to every response.
pub async fn a2a_version_middleware(
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    let path = request.uri().path();

    // Skip version checks for non-A2A paths (health checks, agent card discovery).
    let is_a2a_path = !matches!(
        path,
        "/health" | "/.well-known/agent-card.json" | "/extendedAgentCard"
    );

    if is_a2a_path {
        let version = request
            .headers()
            .get("A2A-Version")
            .and_then(|v| v.to_str().ok());

        match version {
            Some(v) if v == crate::A2A_VERSION => {}
            _ => {
                let error_body = a2a_error(
                    400,
                    "INVALID_REQUEST",
                    "A2A-Version header required. Supported: 1.0",
                );
                return (
                    StatusCode::BAD_REQUEST,
                    [("A2A-Version", "1.0")],
                    Json(error_body),
                )
                    .into_response();
            }
        }
    }

    let mut response = next.run(request).await;
    let _ = response.headers_mut().insert(
        "A2A-Version",
        // A2A_VERSION is a compile-time constant, so from_static is infallible.
        axum::http::HeaderValue::from_static(crate::A2A_VERSION),
    );
    response
}
