//! Shared fixtures for the transcript test modules.
//!
//! Holds the imports and the helpers every topical transcript test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::state::{AppState, ToolCallLine, ToolCallStatus};
pub(super) use agnt_core::protocol::contracts::UiMessageSnapshot;
pub(super) use agnt_core::protocol::event::{ToolDisplay, ToolDisplaySection};
pub(super) use agnt_core::protocol::events::ToolCall;
pub(super) use agnt_core::protocol::tool_args::CallLine;
pub(super) use agnt_core::transcript::entry::{MessageEntry, ToolEntry, ToolKind, TranscriptEntry};
pub(super) use agnt_core::transcript::ir::{
    ContentKind, Display, DisplaySection, MessageRole, NoticeKind, ToolName,
};
pub(super) use agnt_core::transcript::renderer::ItemStatus;

pub(super) use crate::rendering::theme::TuiTheme;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The content-block cap defined in production code. Tests reference this to
/// avoid magic numbers.
pub(super) const MAX_TRANSCRIPT_ENTRIES: usize = 2000;

/// Every entry the store holds is a content entry — the store never inserts
/// Spacer entries (separator rows are a render-time concern).
/// A store that inserted separators would fail this count.
pub(super) fn content_entry_count(state: &AppState) -> usize {
    state.transcript.entries().len()
}

pub(super) fn push_user(state: &mut AppState, text: &str) {
    let entry = TranscriptEntry::Message(MessageEntry {
        role: MessageRole::User,
        markdown: text.to_string(),
    });
    state.transcript.push_entry(entry);
}

pub(super) fn push_assistant(state: &mut AppState, text: &str) {
    let entry = TranscriptEntry::Message(MessageEntry {
        role: MessageRole::Assistant,
        markdown: text.to_string(),
    });
    state.transcript.push_entry(entry);
}

/// Row count the message widget produces for a user prose entry at `width`.
pub(super) fn rendered_row_count(markdown: &str, width: usize) -> usize {
    let entry = TranscriptEntry::Message(MessageEntry {
        role: MessageRole::User,
        markdown: markdown.to_string(),
    });
    let probe = crate::widgets::RatatuiBackend::new(u16::try_from(width).unwrap_or(u16::MAX), 0);
    agnt_render::RenderBackend::measure(&probe, &entry, width)
}

/// Minimal tool entry for cap-boundary tests: carries no meaningful content.
pub(super) fn simple_tool_entry() -> TranscriptEntry {
    let call = ToolCall::new(
        ToolName("read".to_string()),
        "",
        "",
        CallLine {
            summary: String::new(),
        },
    );
    TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::from_name(&call.name),
        call,
        preview: None,
        result: None,
        status: ItemStatus::InProgress,
    })
}
