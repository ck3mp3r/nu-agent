use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::{Value, json};

use crate::PushAuthenticationInfo;

use super::super::AppState;
use super::super::response::{a2a_error, a2a_json_response};

pub async fn handle_create_push_config(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let url = body.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        let err = a2a_error(400, "BAD_REQUEST", "Missing required field: 'url'");
        return (StatusCode::BAD_REQUEST, a2a_json_response(err));
    }
    let auth = body
        .get("authentication")
        .and_then(|v| serde_json::from_value::<PushAuthenticationInfo>(v.clone()).ok());
    let config = state.task_store.create_push_config(&id, url, auth);
    (StatusCode::OK, a2a_json_response(json!(config)))
}

pub async fn handle_list_push_configs(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let configs = state.task_store.list_push_configs(&id);
    (
        StatusCode::OK,
        a2a_json_response(json!({ "configs": configs })),
    )
}

pub async fn handle_delete_push_config(
    State(state): State<AppState>,
    axum::extract::Path((id, config_id)): axum::extract::Path<(String, String)>,
) -> impl IntoResponse {
    state.task_store.delete_push_config(&id, &config_id);
    (StatusCode::OK, a2a_json_response(json!({})))
}
