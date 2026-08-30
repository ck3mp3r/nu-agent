use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
};

use super::super::AppState;

pub async fn handle_agent_card(State(state): State<AppState>) -> impl IntoResponse {
    // Clone the fields we need and drop the read guard before doing any
    // fallible work (serialization, header parsing) so concurrent writers are
    // not blocked while we assemble the response.
    let (card_value, version) = {
        let card = state.agent_card.read().expect("agent_card lock");
        ((*card).clone(), card.version.clone())
    };

    // Errors here can only arise from an invalid card value or invalid header
    // value; the card comes from in-process config, so treat a failure as a
    // 500 rather than panicking.
    let card_value = match serde_json::to_value(&card_value) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("serialize AgentCard: {e}")})),
            )
                .into_response();
        }
    };
    match format!("\"{version}\"").parse::<HeaderValue>() {
        Ok(etag) => {
            let mut headers = HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/a2a+json"),
            );
            headers.insert(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("max-age=300"),
            );
            headers.insert(axum::http::header::ETAG, etag);
            (headers, Json(card_value)).into_response()
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "card version produces an invalid ETag header value"
            })),
        )
            .into_response(),
    }
}

pub async fn handle_extended_agent_card(State(state): State<AppState>) -> impl IntoResponse {
    let card = state.agent_card.read().expect("agent_card lock");
    let card_value = serde_json::to_value(&*card).unwrap_or_default();
    drop(card);
    let extended = serde_json::json!({
        "agentCard": card_value,
        "extendedCapabilities": {
            "streaming": true,
            "pushNotifications": false,
            "subscribeToTask": true,
            "listTasks": true,
        },
        "provider": {
            "organization": "nu-agent",
            "url": "https://github.com/ck3mp3r/nu-agent",
        },
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        // Static constant; from_static is infallible for a valid value.
        HeaderValue::from_static("application/a2a+json"),
    );
    (headers, Json(extended))
}
