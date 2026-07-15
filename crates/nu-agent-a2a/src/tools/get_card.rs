use serde_json::Value;

use super::{A2aToolContext, ToolResult};

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
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP client creation failed: {e}"))?;

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(serde_json::json!({
                "name": name,
                "error": format!("card fetch failed: {e}")
            }));
        }
    };

    if !resp.status().is_success() {
        return Ok(serde_json::json!({
            "name": name,
            "error": format!("card fetch failed: HTTP {}", resp.status().as_u16())
        }));
    }

    match resp.json::<serde_json::Value>().await {
        Ok(card) => Ok(card),
        Err(e) => Ok(serde_json::json!({
            "name": name,
            "error": format!("card fetch failed: bad JSON: {e}")
        })),
    }
}
