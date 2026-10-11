use axum::{Json, extract::State, http::StatusCode, middleware::Next, response::IntoResponse};

use super::AppState;
use super::response::a2a_error;

// ---------------------------------------------------------------------------
// A2A-Version middleware (A2A spec §9.2, §14.2)
// ---------------------------------------------------------------------------

/// Axum middleware that validates the incoming `A2A-Version` and
/// `A2A-Extensions` headers on A2A API paths and adds the `A2A-Version` header
/// to every response.
///
/// `A2A-Extensions` (spec §3.2.6, §14.2.2) is a comma-separated list of
/// extension URIs. Every declared URI must appear in the agent card's
/// `extensions` list; otherwise the request is rejected.
pub async fn a2a_version_middleware(
    State(state): State<AppState>,
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
                    "VERSION_NOT_SUPPORTED",
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

        // Validate declared extensions against the agent card (spec §3.2.6).
        if let Some(header) = request
            .headers()
            .get("A2A-Extensions")
            .and_then(|v| v.to_str().ok())
        {
            let declared: Vec<&str> = header
                .split(',')
                .map(str::trim)
                .filter(|uri| !uri.is_empty())
                .collect();

            let unsupported: Vec<&str> = {
                let card = state.agent_card.read().expect("agent_card lock");
                declared
                    .iter()
                    .copied()
                    .filter(|uri| !card.extensions.iter().any(|e| e.as_str() == *uri))
                    .collect()
            };

            if !unsupported.is_empty() {
                let error_body = a2a_error(
                    400,
                    "INVALID_REQUEST",
                    "UNSUPPORTED_OPERATION",
                    &format!("Unsupported A2A extensions: {}", unsupported.join(", ")),
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
