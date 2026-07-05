use std::path::Path;

use super::ToolHandlerError;

/// Handle send_message tool invocation using socket dir (for children)
pub async fn handle_send_message(
    args: &serde_json::Value,
    socket_dir: &Path,
    from: &str,
) -> Result<serde_json::Value, ToolHandlerError> {
    let to = args["to"]
        .as_str()
        .ok_or_else(|| ToolHandlerError::runtime("Missing required field: to"))?;
    let message = args["message"]
        .as_str()
        .ok_or_else(|| ToolHandlerError::runtime("Missing required field: message"))?;
    let kind = args["kind"].as_str().unwrap_or("message");

    crate::mailbox::send_to(socket_dir, to, from, message, kind)
        .await
        .map_err(|e| ToolHandlerError::runtime(format!("Failed to send message: {e}")))?;

    Ok(serde_json::json!({ "sent": true }))
}

/// Dispatch send_message using socket dir (for children)
pub async fn dispatch_send_message(
    arguments: &serde_json::Value,
    socket_dir: &Path,
    from: &str,
) -> Result<Option<serde_json::Value>, ToolHandlerError> {
    handle_send_message(arguments, socket_dir, from)
        .await
        .map(Some)
}

#[cfg(test)]
#[path = "messaging_test.rs"]
mod messaging_test;
