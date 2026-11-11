use serde_json::Value;

use super::{A2aToolContext, ToolResult};
use crate::client::A2aHttpClient;

pub async fn handle(ctx: A2aToolContext, params: Value) -> ToolResult {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'name'".to_string())?;

    let peer = ctx
        .cache
        .get(name)
        .ok_or_else(|| format!("Agent '{name}' not found"))?;

    let url = format!("{}/.well-known/agent-card.json", peer.url);
    let body = match ctx
        .client
        .get_bytes_with_timeout(&url, std::time::Duration::from_secs(5))
        .await
    {
        Ok(bytes) => bytes,
        Err(e) => {
            return Ok(serde_json::json!({
                "name": name,
                "error": format!("card fetch failed: {e}")
            }));
        }
    };

    match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(card) => Ok(card),
        Err(e) => Ok(serde_json::json!({
            "name": name,
            "error": format!("card fetch failed: bad JSON: {e}")
        })),
    }
}
