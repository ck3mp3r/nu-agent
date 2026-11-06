//! Shared fixtures for the tool state test modules.
//!
//! Holds the imports and the helper functions every topical tool test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::{apply_permission_request_display, reduce};
pub(super) use crate::state::{AppState, PermissionPrompt};
pub(super) use crate::test_support::entry_text;
pub(super) use agnt_core::protocol::contracts::UiMessageSnapshot;
pub(super) use agnt_core::protocol::events::{SystemEvent, ToolCall, ToolCallId, ToolOutcome};
pub(super) use agnt_core::session::tool_status::ToolStatus;
pub(super) use agnt_render::tool_view::ToolView;
pub(super) use agnt_render::transcript::entry::{MessageEntry, ToolEntry, TranscriptEntry};
pub(super) use agnt_render::transcript::ir::StyleHint;
pub(super) use agnt_render::transcript::ir::ToolName;
pub(super) use agnt_render::transcript::ir::{
    ContentKind, DiffTint, Display, DisplaySection, MessageRole,
};
pub(super) use agnt_render::transcript::renderer::ItemStatus;
pub(super) use agnt_runtime::protocol::event::PermissionRequestContext;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// The unified display of a tool entry: `result.or(preview)`, the one slot the
/// backends render.
fn unified_display(tool: &ToolEntry) -> Option<&Display> {
    tool.result.as_ref().or(tool.preview.as_ref())
}

/// The tool display content of one entry: a tool entry's unified display
/// (`result.or(preview)`); every other family contributes nothing.
fn entry_display_lines(entry: &TranscriptEntry) -> Vec<agnt_render::transcript::ir::ContentLine> {
    match entry {
        TranscriptEntry::Tool(tool) => unified_display(tool)
            .map(Display::project_lines)
            .unwrap_or_default(),
        TranscriptEntry::Message(_)
        | TranscriptEntry::Task(_)
        | TranscriptEntry::Notice(_)
        | TranscriptEntry::Banner(_) => Vec::new(),
    }
}

/// Collect the StyleHints of every tool display line the store holds.
pub(super) fn diff_section_hints(state: &AppState) -> Vec<StyleHint> {
    state
        .transcript
        .entries()
        .iter()
        .flat_map(entry_display_lines)
        .flat_map(|line| {
            line.spans
                .iter()
                .map(|span| span.hint.clone())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Collect the line-level diff tints of every tool display line the store
/// holds.
pub(super) fn diff_section_tints(state: &AppState) -> Vec<DiffTint> {
    state
        .transcript
        .entries()
        .iter()
        .flat_map(entry_display_lines)
        .filter_map(|line| line.diff_tint.clone())
        .collect()
}

/// Project a display into one newline-joined text block.
pub(super) fn display_lines(display: &Display) -> String {
    display
        .project_lines()
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The call line the entry's own data derives, as every backend derives it.
pub(super) fn call_line_of(tool: &ToolEntry) -> String {
    tool.call.call_line()
}

/// Flatten one entry's text for assertions. A tool entry contributes its call
/// line, derived from its own data, and its unified display
/// (`result.or(preview)`) — the one slot the backends render.
pub(super) fn extract_all_text_from_entry(entry: &TranscriptEntry) -> Vec<String> {
    match entry {
        TranscriptEntry::Tool(tool) => {
            let mut lines = vec![call_line_of(tool)];
            if let Some(display) = unified_display(tool) {
                lines.extend(display_lines(display).lines().map(str::to_string));
            }
            lines
        }
        TranscriptEntry::Message(message) => vec![message.markdown.clone()],
        TranscriptEntry::Task(task) => vec![format!("{} · {}", task.name, task.summary)],
        TranscriptEntry::Notice(notice) => vec![notice.text.clone()],
        TranscriptEntry::Banner(banner) => vec![banner.text.clone()],
    }
}

/// The tool entry at `index`.
pub(super) fn tool_entry_at(state: &AppState, index: usize) -> Result<&ToolEntry> {
    let entry = state
        .transcript
        .entries()
        .get(index)
        .ok_or("should have entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    Ok(tool)
}

pub(super) fn started(name: &str, arguments: &str) -> SystemEvent {
    SystemEvent::ToolStarted {
        call: ToolCall::new(
            ToolName(name.to_string()),
            agnt_runtime::tools::handler::ToolSource::Mcp,
            arguments,
        ),
    }
}

/// A `ToolCompleted` event with an explicit source and outcome.
pub(super) fn completed_with(
    name: &str,
    source: agnt_runtime::tools::handler::ToolSource,
    arguments: &str,
    outcome: ToolOutcome,
) -> SystemEvent {
    SystemEvent::ToolCompleted {
        call: ToolCall::new(ToolName(name.to_string()), source, arguments),
        outcome,
    }
}

/// Fold one `SystemEvent` through the reducer.
pub(super) fn reduce_ui(state: &mut AppState, event: SystemEvent) -> bool {
    reduce(state, event, None)
}

/// A tool call for `name`/`arguments` with the shared test source.
pub(super) fn tool_call(name: &str, arguments: &str) -> ToolCall {
    ToolCall::new(
        ToolName(name.to_string()),
        agnt_runtime::tools::handler::ToolSource::Mcp,
        arguments,
    )
}

/// The outcome a producer settles on: the result text the model received and
/// the disposition it settled at. Nothing here is drawn — a surface derives
/// the completion body from the call and this text.
pub(super) fn outcome_of(
    status: ToolStatus,
    result: &str,
    error_kind: Option<&str>,
) -> ToolOutcome {
    ToolOutcome {
        success: status == ToolStatus::Success,
        status,
        result: result.to_string(),
        error_kind: error_kind.map(str::to_string),
        message: None,
    }
}

/// Reduce a `Completed` event whose result text is `result`. A surface derives
/// the completion body from that text, exactly as it does live.
pub(super) fn complete_with_result(
    state: &mut AppState,
    name: &str,
    arguments: &str,
    result: &str,
) {
    reduce_ui(
        state,
        completed_with(
            name,
            agnt_runtime::tools::handler::ToolSource::Closure,
            arguments,
            outcome_of(ToolStatus::Success, result, None),
        ),
    );
}

/// The JSON an `edit` handler returns: the path it changed and the diff it
/// made. The tool's own view derives the completion body from these two keys.
pub(super) fn edit_payload(path: &str, diff: &str) -> String {
    // `serde_json` is not a test dependency here: build the handler's own shape.
    let escaped = diff
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("{{\"path\":\"{path}\",\"diff\":\"{escaped}\"}}")
}

/// Build a permission request context with the shared test defaults.
pub(super) fn permission_context(
    tool: &str,
    tool_key: &str,
    preview: Option<&str>,
) -> PermissionRequestContext {
    PermissionRequestContext {
        tool: tool.to_string(),
        tool_key: tool_key.to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        preview: preview.map(str::to_string),
    }
}

// endregion: --- Test Support
