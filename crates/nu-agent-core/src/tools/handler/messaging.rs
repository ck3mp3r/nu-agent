use std::sync::Arc;
use tokio::sync::RwLock;

use crate::mailbox::{AgentRegistry, BrokerSender, ServerFrame};

use super::ToolHandlerError;

/// Handle send_message tool invocation using BrokerSender (for children)
pub async fn handle_send_message(
    args: &serde_json::Value,
    sender: &mut BrokerSender,
) -> Result<serde_json::Value, ToolHandlerError> {
    let to = args["to"]
        .as_str()
        .ok_or_else(|| ToolHandlerError::runtime("Missing required field: to"))?;
    let message = args["message"]
        .as_str()
        .ok_or_else(|| ToolHandlerError::runtime("Missing required field: message"))?;
    let kind = args["kind"].as_str().unwrap_or("message");

    sender
        .send(to, message, kind)
        .await
        .map_err(|e| ToolHandlerError::runtime(format!("Failed to send message: {e}")))?;

    Ok(serde_json::json!({ "sent": true }))
}

/// Handle send_message tool invocation using AgentRegistry (for parent)
pub fn handle_send_message_via_registry(
    args: &serde_json::Value,
    registry: &Arc<RwLock<AgentRegistry>>,
    from: &str,
) -> Result<serde_json::Value, ToolHandlerError> {
    let to = args["to"]
        .as_str()
        .ok_or_else(|| ToolHandlerError::runtime("Missing required field: to"))?;
    let message = args["message"]
        .as_str()
        .ok_or_else(|| ToolHandlerError::runtime("Missing required field: message"))?;
    let kind = args["kind"].as_str().unwrap_or("message");

    let frame = ServerFrame::Message {
        from: from.to_string(),
        message: message.to_string(),
        kind: kind.to_string(),
    };

    registry
        .try_read()
        .map_err(|_| ToolHandlerError::runtime("Failed to acquire registry lock"))?
        .route_message(to, frame)
        .map_err(|e| ToolHandlerError::runtime(format!("Failed to route message: {e}")))?;

    Ok(serde_json::json!({ "sent": true }))
}

/// Dispatch send_message using BrokerSender (for children)
pub async fn dispatch_send_message(
    arguments: &serde_json::Value,
    sender: &mut BrokerSender,
) -> Result<Option<serde_json::Value>, ToolHandlerError> {
    handle_send_message(arguments, sender).await.map(Some)
}

/// Dispatch send_message using AgentRegistry (for parent)
pub fn dispatch_send_message_via_registry(
    arguments: &serde_json::Value,
    registry: &Arc<RwLock<AgentRegistry>>,
    from: &str,
) -> Result<Option<serde_json::Value>, ToolHandlerError> {
    handle_send_message_via_registry(arguments, registry, from).map(Some)
}

/// Handle list_agents tool invocation
pub fn handle_list_agents(
    registry: &Arc<RwLock<AgentRegistry>>,
) -> Result<serde_json::Value, ToolHandlerError> {
    // Use try_read to avoid blocking - this is called from sync context
    let names = registry
        .try_read()
        .map_err(|_| ToolHandlerError::runtime("Failed to acquire registry lock"))?
        .connected_names();
    let agents: Vec<serde_json::Value> = names
        .iter()
        .map(|n| serde_json::json!({ "name": n }))
        .collect();
    Ok(serde_json::json!(agents))
}

/// Dispatch list_agents
pub fn dispatch_list_agents(
    registry: &Arc<RwLock<AgentRegistry>>,
) -> Result<Option<serde_json::Value>, ToolHandlerError> {
    handle_list_agents(registry).map(Some)
}

#[cfg(test)]
#[path = "messaging_test.rs"]
mod messaging_test;
