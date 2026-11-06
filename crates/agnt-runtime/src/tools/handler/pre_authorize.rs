use crate::types::ToolCall;
use agnt_kernel::BuiltinKind;
use serde_json::{Map as JsonMap, Value as JsonValue};

use crate::tools::authz::AskContext;

use super::ToolSource;
use super::builtin_tool::Previewable;
use super::edit::EditTool;
use super::resolve::resolve_fs_path_generic;
use crate::tools::closure::EngineInterfaceLike;

#[derive(Debug, Clone, Default)]
pub struct PreAuthorizeOutput {
    pub ask_context: AskContext,
    /// The preview content only the producer can supply — the edit diff, planned
    /// against the file on disk. The surface derives how to draw it.
    pub preview: Option<String>,
}

/// Build the pre-execution preview for a builtin tool: the edit diff, planned
/// against the target file. No writes, no process spawn. `Some` means the gate
/// has a card for this call; `preview` carries only the content a surface cannot
/// derive from the call's own arguments (a `nu` command needs none).
pub fn pre_authorize_fs_tool(
    kind: Option<BuiltinKind>,
    arguments: &JsonMap<String, JsonValue>,
    cwd: &std::path::Path,
) -> Option<PreAuthorizeOutput> {
    let kind = kind?;
    // `Previewable::preview` takes the arguments as a `JsonValue`.
    let args = JsonValue::Object(arguments.clone());
    // Whether the gate has any card to show at all: `edit` always draws a diff,
    // and a `nu` card is the command itself, so a call without one has none.
    let shows_preview = match kind {
        BuiltinKind::Edit => true,
        BuiltinKind::Nu => arguments
            .get("command")
            .and_then(JsonValue::as_str)
            .is_some_and(|command| !command.is_empty()),
        _ => return None,
    };
    if !shows_preview {
        return None;
    }
    let preview = match kind {
        BuiltinKind::Edit => Some(EditTool::preview(&args, cwd)?),
        _ => None,
    };

    Some(PreAuthorizeOutput {
        ask_context: AskContext {
            preview: preview.clone(),
            shows_preview: true,
        },
        preview,
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
