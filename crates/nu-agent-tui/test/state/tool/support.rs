//! Shared fixtures for the tool state test modules.
//!
//! Holds the imports and the helper functions every topical tool test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use crate::interaction::reducer::{apply_permission_request_display, dispatch_ui_event};
pub(super) use crate::rendering::theme::TuiTheme;
pub(super) use crate::state::AppState;
pub(super) use nu_agent_core::bus::ToolEvent;
pub(super) use nu_agent_core::protocol::contracts::UiMessageSnapshot;
pub(super) use nu_agent_core::protocol::event::{ToolDisplay, ToolDisplaySection, UiEvent};
pub(super) use nu_agent_core::protocol::tool_args::CallLine;
pub(super) use nu_agent_core::transcript::ir::Fill;
pub(super) use nu_agent_core::transcript::ir::StyleHint;
pub(super) use nu_agent_core::transcript::ir::{BlockSource, ContentKind, DiffTint};
pub(super) use nu_agent_core::transcript::ir::{Display, DisplaySection};
pub(super) use nu_agent_core::transcript::renderer::ItemStatus;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Collect the StyleHints of all ToolDisplay blocks' stored ContentLines —
/// one block per section, pushed with pre-projected spans.
pub(super) fn diff_section_hints(state: &AppState) -> Vec<StyleHint> {
    state
        .transcript
        .blocks()
        .iter()
        .filter_map(|b| match &b.source {
            BlockSource::ToolDisplay { lines } => Some(lines.clone()),
            _ => None,
        })
        .flat_map(|lines| {
            lines
                .iter()
                .flat_map(|line| line.spans.iter().map(|s| s.hint.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Collect the line-level diff tints of all ToolDisplay blocks' stored
/// ContentLines.
pub(super) fn diff_section_tints(state: &AppState) -> Vec<DiffTint> {
    state
        .transcript
        .blocks()
        .iter()
        .filter_map(|b| match &b.source {
            BlockSource::ToolDisplay { lines } => Some(lines.clone()),
            _ => None,
        })
        .flat_map(|lines| {
            lines
                .iter()
                .filter_map(|line| line.diff_tint.clone())
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn tool_display_lines(block: &nu_agent_core::transcript::ir::Block) -> String {
    match &block.source {
        BlockSource::ToolDisplay { lines } => lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Flatten a block's source text for assertions. A Tool block contributes its
/// call summary only — previews are their own ToolDisplay blocks.
pub(super) fn extract_all_text_from_entry(
    block: &nu_agent_core::transcript::ir::Block,
) -> Vec<String> {
    match &block.source {
        BlockSource::Tool { call, .. } => vec![call.summary.clone()],
        BlockSource::ToolDisplay { lines } => lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<String>()
            })
            .collect(),
        BlockSource::Notice { text, .. } => vec![text.clone()],
        BlockSource::Markdown { markdown, .. } => vec![markdown.clone()],
        BlockSource::Banner { text } => vec![text.clone()],
        BlockSource::Spacer => vec![String::new()],
    }
}

pub(super) fn reduce_tool(state: &mut AppState, event: ToolEvent) -> bool {
    let mut evicted = 0usize;
    let changed = state
        .tool
        .reduce_tool_event(&mut state.transcript, event, &mut evicted);
    state.shift_bookkeeping_after_eviction(evicted);
    changed
}

pub(super) fn started(name: &str, arguments: &str) -> ToolEvent {
    ToolEvent::Started {
        name: name.to_string(),
        source: "mcp".to_string(),
        arguments: arguments.to_string(),
        call_line: CallLine::from_json_summary(arguments),
    }
}

pub(super) fn started_with_call_line(
    name: &str,
    arguments: &str,
    call_line: CallLine,
) -> ToolEvent {
    ToolEvent::Started {
        name: name.to_string(),
        source: "mcp".to_string(),
        arguments: arguments.to_string(),
        call_line,
    }
}

// endregion: --- Test Support
