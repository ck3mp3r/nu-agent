use axum::{Json, extract::State, http::HeaderMap, response::IntoResponse};

use super::super::AppState;

pub async fn handle_agent_card(State(state): State<AppState>) -> impl IntoResponse {
    let card = state.agent_card.read().expect("agent_card lock");
    let card_value = serde_json::to_value(&*card).expect("serialize AgentCard");
    let version = card.version.clone();
    drop(card);
    let etag = format!("\"{}\"", version);
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        "application/a2a+json".parse().expect("static content-type"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        "max-age=300"
            .parse()
            .expect("static CACHE_CONTROL value is always valid"),
    );
    headers.insert(
        axum::http::header::ETAG,
        etag.parse()
            .expect("card version must produce a valid ETag header value"),
    );
    (headers, Json(card_value))
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
        "application/a2a+json".parse().expect("static content-type"),
    );
    (headers, Json(extended))
}
