use crate::types::ToolCall;
use nu_plugin::EngineInterface;
use serde_json::Value as JsonValue;

use crate::protocol::event::ToolDisplay;
use crate::tools::authz::AskContext;

use super::ToolSource;

#[derive(Debug, Clone, Default)]
pub struct PreAuthorizeOutput {
    pub ask_context: AskContext,
    pub display: Option<ToolDisplay>,
}

pub fn pre_authorize_builtin_fs_tool(
    tool_name: &str,
    arguments: &JsonValue,
    cwd: &std::path::Path,
) -> Option<PreAuthorizeOutput> {
    if tool_name != "edit" {
        return None;
    }

    let args: super::builtin_fs::BuiltinEditArgs =
        serde_json::from_value(arguments.clone()).ok()?;
    let mode = super::builtin_fs::parse_edit_mode(args.mode.as_deref()).ok()?;
    if mode != super::builtin_fs::EditToolMode::Apply {
        return None;
    }

    let operation = super::builtin_fs::resolve_edit_operation(&args).ok()?;
    let resolved_path = super::builtin_fs::resolve_builtin_fs_path_for_cwd(&args.path, cwd);
    let plan = crate::tools::fs::core::plan_search_replace_edit(
        &resolved_path,
        args.expected_version.as_deref(),
        &operation,
    )
    .ok()?;

    let preview_display = super::result::build_edit_preview_display(
        super::builtin_fs::build_edit_preview_display_payload(&args.path, &plan),
    );
    Some(PreAuthorizeOutput {
        ask_context: AskContext {
            pre_authorize_display: Some(preview_display.clone()),
        },
        display: Some(preview_display),
    })
}

pub fn pre_authorize_tool_call(
    tool_call: &ToolCall,
    source: ToolSource,
    engine: &EngineInterface,
) -> PreAuthorizeOutput {
    match source {
        ToolSource::Closure | ToolSource::Builtin | ToolSource::BuiltinFs => {
            let builtin_cwd = match super::builtin_fs::resolve_builtin_fs_path(".", engine) {
                Ok(path) => path,
                Err(_) => return PreAuthorizeOutput::default(),
            };

            pre_authorize_builtin_fs_tool(
                &tool_call.function.name,
                &tool_call.function.arguments,
                &builtin_cwd,
            )
            .unwrap_or_default()
        }
        ToolSource::Mcp | ToolSource::Unknown => PreAuthorizeOutput::default(),
    }
}
