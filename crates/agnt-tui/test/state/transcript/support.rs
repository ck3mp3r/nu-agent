//! Shared fixtures for the transcript test modules.
//!
//! Holds the imports and the helpers every topical transcript test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use crate::state::{AppState, ToolCallLine, ToolCallStatus};
pub(super) use agnt_core::protocol::contracts::UiMessageSnapshot;
pub(super) use agnt_core::protocol::event::{ToolDisplay, ToolDisplaySection};
pub(super) use agnt_core::protocol::tool_args::CallLine;
pub(super) use agnt_core::transcript::ir::{
    Block, BlockSource, ContentKind, Display, DisplaySection, Fill, Lane, MessageRole, NoticeKind,
    ToolName,
};
pub(super) use agnt_core::transcript::items::{Message, Tool};
pub(super) use agnt_core::transcript::renderer::{FrameContext, ItemStatus, Renderable};

pub(super) use crate::rendering::theme::TuiTheme;

/// Construct a Block from a Renderable item in one expression — the direct
/// construction form the spec mandates for all callers.
pub(super) fn block_from_item(item: &impl Renderable) -> Block {
    Block {
        source: item.source(),
        lane: item.lane(),
        fill: item.fill(),
        status: None,
    }
}

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The block cap that will be defined in production code.
/// Tests reference this to avoid magic numbers.
pub(super) const MAX_TRANSCRIPT_BLOCKS: usize = 2000;

/// Every block the store holds is a content block — the store never inserts
/// Spacer blocks (separator rows are a render-time concern). Counts every
/// block; a store that inserted separators would fail this count.
pub(super) fn content_block_count(state: &AppState) -> usize {
    state.transcript.blocks().len()
}

pub(super) fn push_user(state: &mut AppState, text: &str) {
    let msg = Message {
        role: MessageRole::User,
        markdown: text.to_string(),
    };
    state.transcript.push_block(block_from_item(&msg));
}

pub(super) fn push_assistant(state: &mut AppState, text: &str) {
    let msg = Message {
        role: MessageRole::Assistant,
        markdown: text.to_string(),
    };
    state.transcript.push_block(block_from_item(&msg));
}

/// Row count layout() produces for a user prose block at `width`.
pub(super) fn rendered_row_count(markdown: &str, width: usize) -> usize {
    let msg = Message {
        role: MessageRole::User,
        markdown: markdown.to_string(),
    };
    let block = agnt_core::transcript::ir::Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: Fill::Full,
        status: None,
    };
    let ctx = FrameContext {
        width,
        now_millis: 0,
        cursor: false,
        selected: false,
    };
    crate::tui_renderer::layout(&block, &ctx, &TuiTheme::default()).len()
}

/// Minimal Tool block for cap-boundary tests: carries no meaningful content.
pub(super) fn simple_tool_block() -> Block {
    let tool = Tool {
        name: ToolName("read".to_string()),
        call: CallLine {
            summary: String::new(),
        },
        preview: None,
        result: None,
        status: ItemStatus::InProgress,
    };
    Block {
        source: tool.source(),
        lane: tool.lane(),
        fill: tool.fill(),
        status: Some(ItemStatus::InProgress),
    }
}
