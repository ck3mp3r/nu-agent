//! Shared fixtures for the render transcript test modules.
//!
//! Holds the imports and the helpers every topical transcript test file needs.
//! Sub-files pull them in with `use super::*;`.

pub(super) use super::super::transcript::full_width_background;
pub(super) use agnt_core::protocol::tool_args::CallLine;
pub(super) use agnt_core::transcript::ir::{BlockSource, Fill, Lane, MessageRole, ToolName};
pub(super) use agnt_core::transcript::renderer::FrameContext;

pub(super) use crate::rendering::theme::TuiTheme;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Theme colors used by `full_width_background`; values are opaque — tests
/// assert identity, not specific colors.
pub(super) const FULL_BG: ratatui::style::Color = ratatui::style::Color::Indexed(16);
pub(super) const CODE_BG: ratatui::style::Color = ratatui::style::Color::Indexed(17);

pub(super) fn user_block() -> agnt_core::transcript::ir::Block {
    agnt_core::transcript::ir::Block {
        source: BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        },
        lane: Lane::Marker("▏"),
        fill: Fill::Full,
        status: None,
    }
}

pub(super) fn assistant_block() -> agnt_core::transcript::ir::Block {
    agnt_core::transcript::ir::Block {
        source: BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "hi".to_string(),
        },
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    }
}

pub(super) fn spacer_block() -> agnt_core::transcript::ir::Block {
    agnt_core::transcript::ir::Block {
        source: BlockSource::Spacer,
        lane: Lane::Blank,
        fill: Fill::None,
        status: None,
    }
}

pub(super) fn tool_block(fill: Fill) -> agnt_core::transcript::ir::Block {
    agnt_core::transcript::ir::Block {
        source: BlockSource::Tool {
            name: ToolName("tool".to_string()),
            call: CallLine {
                summary: "→ x".to_string(),
            },
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill,
        status: None,
    }
}

pub(super) fn nu_tool_block(code: &str) -> agnt_core::transcript::ir::Block {
    agnt_core::transcript::ir::Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: code.to_string(),
            },
            preview: Some(agnt_core::transcript::ir::Display {
                title: Some("nu".to_string()),
                sections: vec![agnt_core::transcript::ir::DisplaySection {
                    label: Some("nu".to_string()),
                    kind: agnt_core::transcript::ir::ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: code.to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::Code,
        status: None,
    }
}

pub(super) fn frame_ctx(width: usize) -> FrameContext {
    FrameContext {
        width,
        now_millis: 0,
        cursor: false,
        selected: false,
    }
}

// ---------------------------------------------------------------------------
// Render loop end-to-end (task 7a671267)
// ---------------------------------------------------------------------------

pub(super) use crate::runtime::coordinator::RuntimeCoordinator;
pub(super) use agnt_core::transcript::ir::Block;
pub(super) use agnt_core::transcript::ir::Lane as L;
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

/// Background of the cell at `(x, y)` in the rendered buffer.
pub(super) fn cell_bg(
    terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
    x: u16,
    y: u16,
) -> Option<ratatui::style::Color> {
    terminal.backend().buffer()[(x, y)].style().bg
}
