use axum::{extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;

use super::super::AppState;
use super::super::response::a2a_json_response;

pub async fn handle_file_upload(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let file_id = uuid::Uuid::new_v4().to_string();
    state.write_files().insert(file_id.clone(), body.to_vec());
    let url = {
        let card = state.read_agent_card();
        card.url.clone()
    };
    let resp = json!({
        "id": file_id,
        "url": format!("{url}/files/{file_id}"),
    });
    a2a_json_response(resp)
}

pub async fn handle_file_download(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let files = state.read_files();
    match files.get(&id) {
        Some(data) => (StatusCode::OK, data.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "File not found").into_response(),
    }
}
