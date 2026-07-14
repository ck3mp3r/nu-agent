use serde_json::Value;

use super::{A2aToolContext, ToolResult};

pub async fn handle(ctx: A2aToolContext, _params: Value) -> ToolResult {
    let peers = ctx.cache.list();
    let own_name = &ctx.own_card.name;
    let result: Vec<Value> = peers
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "url": p.url,
                "description": p.card.as_ref().and_then(|c| c.description.as_deref()),
                "skills": p.card.as_ref().map(|c| {
                    c.skills.iter().map(|s| &s.name).collect::<Vec<_>>()
                }),
                "is_self": p.name == *own_name,
            })
        })
        .collect();
    Ok(serde_json::json!({ "agents": result }))
}
