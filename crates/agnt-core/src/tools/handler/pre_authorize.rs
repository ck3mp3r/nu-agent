use crate::types::ToolCall;
use serde_json::Value as JsonValue;

use crate::protocol::event::ToolDisplay;
use crate::tools::authz::AskContext;

use super::ToolSource;
use super::builtin_kinds::BuiltinKind;
use super::builtin_tool::Previewable;
use super::edit::EditTool;
use super::nu::NuTool;
use super::resolve::resolve_fs_path_generic;
use crate::tools::closure::EngineInterfaceLike;

#[derive(Debug, Clone, Default)]
pub struct PreAuthorizeOutput {
    pub ask_context: AskContext,
    pub display: Option<ToolDisplay>,
}

/// Build the pre-execution preview for a builtin tool. `edit` produces its
/// diff; `nu` produces a code block of the command it is about to run. Both
/// previews come straight from the tool type's `Previewable::preview` and
/// read only the tool-call arguments (plus, for `edit`, a planned diff of
/// the target file) — no writes, no process spawn.
pub fn pre_authorize_fs_tool(
    kind: Option<BuiltinKind>,
    arguments: &JsonValue,
    cwd: &std::path::Path,
) -> Option<PreAuthorizeOutput> {
    let preview_display = match kind? {
        BuiltinKind::Edit => EditTool::preview(arguments, cwd)?,
        BuiltinKind::Nu => NuTool::preview(arguments, cwd)?,
        _ => return None,
    };

    Some(PreAuthorizeOutput {
        ask_context: AskContext {
            pre_authorize_display: Some(preview_display.clone()),
        },
        display: Some(preview_display),
    })
}

pub fn pre_authorize_tool_call<E: EngineInterfaceLike>(
    tool_call: &ToolCall,
    source: ToolSource,
    engine: &E,
) -> PreAuthorizeOutput {
    match source {
        ToolSource::Closure | ToolSource::Builtin => {
            let builtin_cwd = match resolve_fs_path_generic(".", engine) {
                Ok(path) => path,
                Err(_) => return PreAuthorizeOutput::default(),
            };

            let kind = tool_call.function.name.parse::<BuiltinKind>().ok();
            pre_authorize_fs_tool(kind, &tool_call.function.arguments, &builtin_cwd)
                .unwrap_or_default()
        }
        ToolSource::Mcp | ToolSource::Unknown => PreAuthorizeOutput::default(),
    }
}

#[cfg(test)]
#[path = "../../../test/tools/handler/pre_authorize.rs"]
mod tests;
