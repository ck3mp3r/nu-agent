use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::harness::{Result, default_tool_infra};
use crate::conversation::turn::executor::*;

// ---------------------------------------------------------------------------
// TestNuShellTool — simple nu__shell tool that returns a fixed result
// ---------------------------------------------------------------------------

struct TestNuShellTool {
    response: &'static str,
}

impl rig::tool::Tool for TestNuShellTool {
    const NAME: &'static str = "nu__shell";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Execute a Nushell command".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Ok(self.response.to_string())
    }
}

/// Register a nu__shell tool that returns a fixed result string.
pub(crate) fn nu_shell_tool(response: &'static str) -> Result<ToolInfra> {
    let handle = rig::tool::server::ToolServer::new()
        .tool(TestNuShellTool { response })
        .run();
    Ok(default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: crate::types::ToolName::new("nu__shell")?,
            description: "Execute a Nushell command".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        }],
    ))
}

// ---------------------------------------------------------------------------
// TestTruncatingNuShellTool — nu__shell tool that applies our truncation logic
// ---------------------------------------------------------------------------

/// A nu__shell tool that goes through `truncate_tool_output` so integration
/// tests can verify the truncation threshold is respected end-to-end.
fn build_truncating_tool(
    response: &'static str,
    max_tool_result_bytes: usize,
) -> Result<rig::tool::DynamicTool> {
    Ok(rig::tool::DynamicTool::new(
        crate::types::ToolName::new("nu__shell")?,
        "Execute a Nushell command",
        serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        move |_args| {
            let output = response.to_string();
            let max_bytes = max_tool_result_bytes;
            Box::pin(async move {
                Ok(rig::tool::ToolOutput::text(
                    crate::tools::limits::truncate_tool_output(output, max_bytes),
                ))
            })
        },
    ))
}

/// Register a nu__shell tool that applies truncation at `max_tool_result_bytes`.
pub(crate) async fn nu_shell_tool_truncating(
    response: &'static str,
    max_tool_result_bytes: usize,
) -> Result<ToolInfra> {
    let handle = rig::tool::server::ToolServer::new().run();
    // Register via add_dynamic_tool
    handle.add_dynamic_tool(build_truncating_tool(response, max_tool_result_bytes)?);
    Ok(default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: crate::types::ToolName::new("nu__shell")?,
            description: "Execute a Nushell command".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        }],
    ))
}

// ---------------------------------------------------------------------------
// TestEchoTool — two named structs because Tool::NAME is a const
// ---------------------------------------------------------------------------

struct TestEchoTool {
    response: &'static str,
}

impl rig::tool::Tool for TestEchoTool {
    const NAME: &'static str = "test_echo";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Test echo tool".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Ok(self.response.to_string())
    }
}

/// Register one echo tool (test_echo) that returns a controlled string.
pub(crate) fn echo_tool(response: &'static str) -> Result<ToolInfra> {
    let handle = rig::tool::server::ToolServer::new()
        .tool(TestEchoTool { response })
        .run();
    Ok(default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: crate::types::ToolName::new("test_echo")?,
            description: "Test echo tool".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }],
    ))
}

// ---------------------------------------------------------------------------
// TestNuShellCancellingTool — cancels the running turn after the first call
// ---------------------------------------------------------------------------

/// A `nu__shell` mock tool that cancels the running turn after producing its result.
///
/// Cancellation fires AFTER `call()` returns, so the tool result is recorded in
/// `new_messages` before the cancel event fires. The cancel takes effect at the next
/// `on_completion_call`'s `is_cancelled()` check (sub-turn 2), not mid-tool.
///
/// Using `tokio::task::yield_now()` ensures the tool result is committed to the
/// `new_messages` list before the cancel event is published.
struct TestNuShellCancellingTool {
    output: &'static str,
    bus: crate::bus::Bus,
    fired: Arc<AtomicBool>,
}

impl rig::tool::Tool for TestNuShellCancellingTool {
    const NAME: &'static str = "nu__shell";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Execute a Nushell command".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        let result = self.output.to_string();
        // Cancel AFTER the tool result is produced. The select! in FilteredToolProxy
        // has already resolved with Ok(result). The cancel event takes effect at the next
        // on_completion_call's is_cancelled() check — AFTER the tool result is recorded.
        if !self.fired.swap(true, Ordering::SeqCst) {
            tokio::task::yield_now().await;
            let _ = self
                .bus
                .cancel()
                .send(crate::bus::CancelEvent::Requested)
                .await;
        }
        Ok(result)
    }
}

/// Register a nu__shell tool that cancels the running turn after its first invocation.
///
/// The `bus` is the same bus the turn executor uses for cancellation.
pub(crate) fn nu_shell_cancelling_tool(
    output: &'static str,
    bus: crate::bus::Bus,
) -> Result<ToolInfra> {
    let handle = rig::tool::server::ToolServer::new()
        .tool(TestNuShellCancellingTool {
            output,
            bus: bus.clone(),
            fired: Arc::new(AtomicBool::new(false)),
        })
        .run();
    let mut infra = default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: crate::types::ToolName::new("nu__shell")?,
            description: "Execute a Nushell command".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        }],
    );
    infra.bus = bus;
    Ok(infra)
}
