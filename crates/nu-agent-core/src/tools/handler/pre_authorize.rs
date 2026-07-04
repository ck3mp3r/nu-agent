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

pub fn pre_authorize_fs_tool(
    tool_name: &str,
    arguments: &JsonValue,
    cwd: &std::path::Path,
) -> Option<PreAuthorizeOutput> {
    if tool_name != "edit" {
        return None;
    }

    let args: super::fs::EditArgs = serde_json::from_value(arguments.clone()).ok()?;
    let mode = super::fs::parse_edit_mode(args.mode.as_deref()).ok()?;
    if mode != super::fs::EditToolMode::Apply {
        return None;
    }

    let operation = super::fs::resolve_edit_operation(&args).ok()?;
    let resolved_path = super::fs::resolve_fs_path_for_cwd(&args.path, cwd);
    let plan = match &operation {
        super::fs::ResolvedEditOperation::SearchReplace(sr_op) => {
            crate::tools::fs::core::plan_search_replace_edit(
                &resolved_path,
                args.expected_version.as_deref(),
                sr_op,
            )
            .ok()?
        }
        super::fs::ResolvedEditOperation::Create { content } => {
            if !resolved_path.parent().is_some_and(|p| p.exists()) {
                return None;
            }
            crate::tools::fs::core::plan_create_file(&resolved_path, content).ok()?
        }
    };

    let preview_display = super::result::build_edit_preview_display(
        super::fs::build_edit_preview_display_payload(&args.path, &plan),
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
            let builtin_cwd = match super::fs::resolve_fs_path(".", engine) {
                Ok(path) => path,
                Err(_) => return PreAuthorizeOutput::default(),
            };

            pre_authorize_fs_tool(
                &tool_call.function.name,
                &tool_call.function.arguments,
                &builtin_cwd,
            )
            .unwrap_or_default()
        }
        ToolSource::Mcp | ToolSource::Unknown => PreAuthorizeOutput::default(),
    }
}
