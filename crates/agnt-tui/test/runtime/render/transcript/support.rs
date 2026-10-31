//! Shared fixtures for the render transcript test modules.
//!
//! Holds the imports and the helpers every topical transcript test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use agnt_core::protocol::events::ToolCall;
pub(super) use agnt_core::protocol::tool_args::CallLine;
pub(super) use agnt_core::transcript::entry::{
    BannerEntry, MessageEntry, ToolEntry, ToolKind, TranscriptEntry,
};
pub(super) use agnt_core::transcript::ir::{
    ContentKind, Display, DisplaySection, MessageRole, ToolName,
};
pub(super) use agnt_core::transcript::renderer::ItemStatus;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A banner entry (startup ASCII art).
pub(super) fn banner_entry(text: &str) -> TranscriptEntry {
    TranscriptEntry::Banner(BannerEntry {
        text: text.to_string(),
    })
}

/// A user prose entry.
pub(super) fn user_entry(text: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role: MessageRole::User,
        markdown: text.to_string(),
    })
}

/// An assistant prose entry.
pub(super) fn assistant_entry(text: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role: MessageRole::Assistant,
        markdown: text.to_string(),
    })
}

/// A plain tool entry: a call line with no preview.
pub(super) fn tool_entry(name: &str, summary: &str) -> TranscriptEntry {
    let call = ToolCall::new(
        ToolName(name.to_string()),
        "",
        "",
        CallLine {
            summary: summary.to_string(),
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

/// The nu tool entry: a code preview (and so the code fill).
pub(super) fn nu_tool_entry(code: &str) -> TranscriptEntry {
    let call = ToolCall::new(
        ToolName("nu".to_string()),
        "",
        "",
        CallLine {
            summary: code.to_string(),
        },
    );
    TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::from_name(&call.name),
        call,
        preview: Some(Display {
            title: Some("nu".to_string()),
            sections: vec![DisplaySection {
                label: Some("nu".to_string()),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: code.to_string(),
                stats: None,
            }],
        }),
        result: None,
        status: ItemStatus::InProgress,
    })
}

// ---------------------------------------------------------------------------
// Render loop end-to-end (task 7a671267)
// ---------------------------------------------------------------------------

pub(super) use crate::runtime::coordinator::RuntimeCoordinator;
pub(super) use ratatui::layout::Rect;

/// Render the transcript pane through `render_transcript_pane` onto a
/// `TestBackend` and return the terminal for buffer inspection. Mirrors the
/// production coordinator layout: the list area equals the content area with
/// the width inset by 2 (1 margin column each side).
pub(super) fn render_transcript_pane_to_backend(
    coord: &mut RuntimeCoordinator,
    columns: u16,
    rows: u16,
    following_tail: bool,
    scroll_offset: usize,
) -> ratatui::Terminal<ratatui::backend::TestBackend> {
    let backend = ratatui::backend::TestBackend::new(columns, rows);
    let mut terminal = match ratatui::Terminal::new(backend) {
        Ok(terminal) => terminal,
        // TestBackend terminal construction is infallible: the error type is
        // uninhabited.
        Err(never) => match never {},
    };
    let content = Rect {
        x: 0,
        y: 0,
        width: columns,
        height: rows,
    };
    let list = Rect {
        width: columns.saturating_sub(2),
        ..content
    };
    let draw_result = terminal.draw(|frame| {
        let mut rendered_scroll_offset = None;
        coord.render_transcript_pane(
            frame,
            content,
            list,
            following_tail,
            scroll_offset,
            &mut rendered_scroll_offset,
        );
    });
    // TestBackend draw is infallible; satisfy the no-unwrap policy without a
    // match on an uninhabited error type.
    if let Err(never) = draw_result {
        match never {}
    }
    terminal
}

/// Symbol of the cell at `(x, y)` in the rendered buffer.
pub(super) fn cell_symbol(
    terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
    x: u16,
    y: u16,
) -> String {
    terminal.backend().buffer()[(x, y)].symbol().to_string()
}

/// Background of the cell at `(x, y)` in the rendered buffer.
pub(super) fn cell_bg(
    terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
    x: u16,
    y: u16,
) -> Option<ratatui::style::Color> {
    terminal.backend().buffer()[(x, y)].style().bg
}
