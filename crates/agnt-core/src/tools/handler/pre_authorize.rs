use crate::types::ToolCall;
use serde_json::{Map as JsonMap, Value as JsonValue};

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
    arguments: &JsonMap<String, JsonValue>,
    cwd: &std::path::Path,
) -> Option<PreAuthorizeOutput> {
    // `Previewable::preview` takes the arguments as a `JsonValue`.
    let args = JsonValue::Object(arguments.clone());
    let preview_display = match kind? {
        BuiltinKind::Edit => EditTool::preview(&args, cwd)?,
        BuiltinKind::Nu => NuTool::preview(&args, cwd)?,
        _ => return None,
    };

    Some(PreAuthorizeOutput {
        ask_context: AskContext {
            pre_authorize_display: Some(preview_display.clone()),
        },
        display: Some(preview_display),
    })
}

/// Reconstruct the argument-sourced preview for a tool call whose result
/// carries no display. Mirrors the live permission-gate path: parse the tool
/// name into a `BuiltinKind`, then ask `pre_authorize_fs_tool` for the
/// preview. Returns `None` for unparseable arguments, unknown tool names,
/// and non-Previewable tools — the caller then emits no display snapshot.
pub fn tool_display_from_arguments(
    tool_name: &str,
    arguments_json: &str,
    cwd: &std::path::Path,
) -> Option<ToolDisplay> {
    let arguments: serde_json::Value = serde_json::from_str(arguments_json).ok()?;
    let kind = tool_name.parse::<BuiltinKind>().ok();
    pre_authorize_fs_tool(kind, arguments.as_object()?, cwd).and_then(|output| output.display)
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
