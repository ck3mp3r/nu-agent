use super::harness::{Result, default_tool_infra};
use crate::conversation::turn::executor::*;

// ---------------------------------------------------------------------------
// Trace-logging fixture
// ---------------------------------------------------------------------------

static TRACE_LOGGER_INSTALL: std::sync::Once = std::sync::Once::new();

/// Open the log gate to `Trace` exactly once per test binary, and install no
/// logger.
///
/// Without an open max level, `log::max_level()` is Off and `log_enabled!` skips
/// the preview construction in `on_outcome`, so the byte-slice defect stays
/// latent. The gate is all this fixture needs: records flow to whatever logger
/// the process holds (under the plain suite, none — a noop by another name), and
/// installing here would contend for the process's one logger slot.
pub(crate) fn install_trace_logger() {
    TRACE_LOGGER_INSTALL.call_once(|| {
        log::set_max_level(log::LevelFilter::Trace);
    });
}

/// A `nu__shell` mock tool that returns a multi-byte UTF-8 result whose byte
/// 2000 falls inside a 2-byte char — the exact shape that panicked the
/// trace-log preview in `HookChain::on_outcome` before the fix.
pub(crate) async fn nu_shell_multibyte_tool() -> Result<ToolInfra> {
    let handle = rig::tool::server::ToolServer::new().run();
    let tool = rig::tool::DynamicTool::new(
        crate::types::ToolName::new("nu__shell")?,
        "Execute a Nushell command",
        serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        move |_args| {
            let output = format!("{}é rest of the output", "a".repeat(1999));
            Box::pin(async move { Ok(rig::tool::ToolOutput::text(output)) })
        },
    );
    handle.add_dynamic_tool(tool);
    Ok(default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            annotations: crate::types::ToolAnnotations::default(),
            name: crate::types::ToolName::new("nu__shell")?,
            description: "Execute a Nushell command".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        }],
    ))
}
