//! Shared fixtures for the widget suite.

use agnt_core::protocol::event::ToolDisplayStats;
use agnt_core::protocol::events::ToolCall;
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::transcript::entry::{
    BannerEntry, MessageEntry, NoticeEntry, ToolEntry, ToolKind, TranscriptEntry,
};
use agnt_core::transcript::ir::{
    ContentKind, Display, DisplaySection, MessageRole, NoticeKind, ToolName,
};
use agnt_core::transcript::renderer::ItemStatus;
use agnt_render::{Color, RenderBackend, RowRect, Theme};

use crate::widgets::RatatuiBackend;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Pane width every fixture renders at.
pub(super) const WIDTH: u16 = 40;
/// Frame height every fixture renders at.
pub(super) const HEIGHT: u16 = 16;

/// A theme with distinct colors, so every assertion names one surface.
pub(super) fn test_theme() -> Theme {
    Theme {
        base: Color::rgb(1, 1, 1),
        text: Color::rgb(2, 2, 2),
        muted: Color::rgb(3, 3, 3),
        idle: Color::rgb(9, 9, 9),
        accent: Color::rgb(4, 4, 4),
        user_accent: Color::rgb(11, 11, 11),
        running: Color::rgb(12, 12, 12),
        success: Color::rgb(7, 7, 7),
        error: Color::rgb(8, 8, 8),
        user_surface: Color::rgb(10, 10, 10),
        code_surface: Color::rgb(20, 20, 20),
        notice: Color::rgb(5, 5, 5),
        banner: Color::rgb(6, 6, 6),
        diff_add_bg: Color::rgb(30, 30, 30),
        diff_remove_bg: Color::rgb(31, 31, 31),
        ..Theme::default()
    }
}

/// A backend with a blank frame at the fixture size.
pub(super) fn blank_backend() -> RatatuiBackend {
    RatatuiBackend::new(WIDTH, HEIGHT)
}

/// Render one entry at `row` and return the backend plus the row count the
/// backend measured.
pub(super) fn render_at(entry: &TranscriptEntry, row: usize) -> Result<(RatatuiBackend, usize)> {
    let mut backend = blank_backend();
    let width = usize::from(WIDTH);
    let rows = backend.measure(entry, width);
    backend.render(entry, RowRect::new(row, width, rows), &test_theme());
    Ok((backend, rows))
}

/// Render one entry at row 0.
pub(super) fn render(entry: &TranscriptEntry) -> Result<RatatuiBackend> {
    Ok(render_at(entry, 0)?.0)
}

// region:    --- Cell reads

/// The symbols of one row, left to right.
pub(super) fn row_text(backend: &RatatuiBackend, y: u16) -> String {
    (0..WIDTH)
        .map(|x| backend.buffer()[(x, y)].symbol().to_string())
        .collect()
}

/// The background of one cell.
pub(super) fn cell_bg(backend: &RatatuiBackend, x: u16, y: u16) -> Option<ratatui::style::Color> {
    backend.buffer()[(x, y)].style().bg
}

/// Whether a cell carries no background. An unpainted cell reports the reset
/// color, so "untinted" covers both the absent and the reset state.
pub(super) fn is_untinted(bg: Option<ratatui::style::Color>) -> bool {
    matches!(bg, None | Some(ratatui::style::Color::Reset))
}

/// The foreground of one cell.
pub(super) fn cell_fg(backend: &RatatuiBackend, x: u16, y: u16) -> Option<ratatui::style::Color> {
    backend.buffer()[(x, y)].style().fg
}

/// A backend-neutral color as the terminal color the backend paints.
pub(super) fn terminal(color: Color) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(color.r, color.g, color.b)
}

// endregion: --- Cell reads

// region:    --- Fixtures

pub(super) fn message_entry(role: MessageRole, markdown: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role,
        markdown: markdown.to_string(),
    })
}

pub(super) fn tool_entry(
    preview: Option<Display>,
    result: Option<Display>,
    status: ItemStatus,
) -> TranscriptEntry {
    TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::Generic,
        call: ToolCall::new(
            ToolName("read".to_string()),
            "",
            "{}",
            CallLine {
                summary: "→ a.rs".to_string(),
            },
        ),
        preview,
        result,
        status,
    })
}

pub(super) fn plain_display(content: &str) -> Display {
    Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Plain,
            content: content.to_string(),
            stats: None,
        }],
    }
}

pub(super) fn code_display(language: &str, content: &str) -> Display {
    Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Code {
                language: language.to_string(),
            },
            content: content.to_string(),
            stats: None,
        }],
    }
}

pub(super) fn diff_display(content: &str) -> Display {
    Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path: "a.rs".to_string(),
            },
            content: content.to_string(),
            stats: None,
        }],
    }
}

pub(super) fn stat_display() -> Display {
    Display {
        title: None,
        sections: vec![DisplaySection {
            label: None,
            kind: ContentKind::Plain,
            content: "body".to_string(),
            stats: Some(ToolDisplayStats {
                files_changed: Some(1),
                insertions: Some(2),
                deletions: Some(0),
                ..ToolDisplayStats::default()
            }),
        }],
    }
}

pub(super) fn notice_entry(kind: NoticeKind, text: &str) -> TranscriptEntry {
    TranscriptEntry::Notice(NoticeEntry {
        kind,
        text: text.to_string(),
    })
}

pub(super) fn banner_entry(text: &str) -> TranscriptEntry {
    TranscriptEntry::Banner(BannerEntry {
        text: text.to_string(),
    })
}

// endregion: --- Fixtures
