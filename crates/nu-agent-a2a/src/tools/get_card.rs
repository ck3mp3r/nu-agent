use serde_json::Value;

use super::{A2aToolContext, ToolResult};

pub async fn handle(ctx: A2aToolContext, params: Value) -> ToolResult {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter: 'name'".to_string())?;

    match ctx.cache.get(name) {
        Some(peer) => {
            let card = match peer.card {
                Some(ref c) => serde_json::to_value(c).unwrap_or_else(
                    |e| serde_json::json!({"error": format!("card serialization failed: {e}")}),
                ),
                None => serde_json::json!({"name": peer.name}),
            };
            Ok(card)
        }
        None => Err(format!("Agent '{name}' not found")),
    }
}
