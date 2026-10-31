//! Shared fixtures for the tool state test modules.
//!
//! Holds the imports and the helper functions every topical tool test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::{apply_permission_request_display, reduce};
pub(super) use crate::state::{AppState, PermissionPrompt};
pub(super) use crate::test_support::entry_text;
pub(super) use agnt_core::protocol::contracts::UiMessageSnapshot;
pub(super) use agnt_core::protocol::event::{ToolDisplay, ToolDisplaySection};
pub(super) use agnt_core::protocol::events::{SystemEvent, ToolCall, ToolCallId, ToolOutcome};
pub(super) use agnt_core::protocol::tool_args::CallLine;
pub(super) use agnt_core::session::tool_status::ToolStatus;
pub(super) use agnt_core::transcript::entry::{MessageEntry, ToolEntry, TranscriptEntry};
pub(super) use agnt_core::transcript::ir::StyleHint;
pub(super) use agnt_core::transcript::ir::ToolName;
pub(super) use agnt_core::transcript::ir::{ContentKind, DiffTint, MessageRole};
pub(super) use agnt_core::transcript::ir::{Display, DisplaySection};
pub(super) use agnt_core::transcript::renderer::ItemStatus;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// The unified display of a tool entry: `result.or(preview)`, the one slot the
/// backends render.
fn unified_display(tool: &ToolEntry) -> Option<&Display> {
    tool.result.as_ref().or(tool.preview.as_ref())
}

/// The tool display content of one entry: a tool entry's unified display
/// (`result.or(preview)`); every other family contributes nothing.
fn entry_display_lines(entry: &TranscriptEntry) -> Vec<agnt_core::transcript::ir::ContentLine> {
    match entry {
        TranscriptEntry::Tool(tool) => unified_display(tool)
            .map(Display::project_lines)
            .unwrap_or_default(),
        TranscriptEntry::Message(_) | TranscriptEntry::Notice(_) | TranscriptEntry::Banner(_) => {
            Vec::new()
        }
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

/// Flatten one entry's text for assertions. A tool entry contributes its call
/// summary and its unified display (`result.or(preview)`) — the one slot the
/// backends render.
pub(super) fn extract_all_text_from_entry(entry: &TranscriptEntry) -> Vec<String> {
    match entry {
        TranscriptEntry::Tool(tool) => {
            let mut lines = vec![tool.call.call_line.summary.clone()];
            if let Some(display) = unified_display(tool) {
                lines.extend(display_lines(display).lines().map(str::to_string));
            }
            lines
        }
        TranscriptEntry::Message(message) => vec![message.markdown.clone()],
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
    started_with_call_line(name, arguments, CallLine::from_json_summary(arguments))
}

pub(super) fn started_with_call_line(
    name: &str,
    arguments: &str,
    call_line: CallLine,
) -> SystemEvent {
    SystemEvent::ToolStarted {
        call: ToolCall::new(ToolName(name.to_string()), "mcp", arguments, call_line),
    }
}

/// A `ToolCompleted` event with an explicit source and outcome.
pub(super) fn completed_with(
    name: &str,
    source: &str,
    arguments: &str,
    outcome: ToolOutcome,
) -> SystemEvent {
    SystemEvent::ToolCompleted {
        call: ToolCall::new(
            ToolName(name.to_string()),
            source,
            arguments,
            CallLine::from_json_summary(arguments),
        ),
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
        "mcp",
        arguments,
        CallLine::from_json_summary(arguments),
    )
}

/// Reduce a `Completed` event carrying an optional display.
pub(super) fn complete_with_display(
    state: &mut AppState,
    name: &str,
    arguments: &str,
    display: Option<ToolDisplay>,
) {
    reduce_ui(
        state,
        completed_with(
            name,
            "closure",
            arguments,
            ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: "{}".to_string(),
                display,
                error_kind: None,
                message: None,
            },
        ),
    );
}

/// Build a diff `ToolDisplaySection` with the shared test path.
pub(super) fn diff_section(label: Option<&str>, content: &str) -> ToolDisplaySection {
    ToolDisplaySection {
        label: label.map(str::to_string),
        kind: ContentKind::Diff {
            language: "diff".to_string(),
            path: "test.rs".to_string(),
        },
        content: content.to_string(),
        stats: None,
    }
}

/// Build a permission request context with the shared test defaults.
pub(super) fn permission_context(
    tool: &str,
    tool_key: &str,
    display: Option<ToolDisplay>,
) -> agnt_core::protocol::event::PermissionRequestContext {
    agnt_core::protocol::event::PermissionRequestContext {
        tool: tool.to_string(),
        tool_key: tool_key.to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: display,
    }
}

// endregion: --- Test Support
