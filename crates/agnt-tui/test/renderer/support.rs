//! Shared fixtures for the renderer suite.
//!
//! Every module paints one entry through the widget backend and reads the
//! buffer, so the assertions name pixels, not intermediate line types.

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

pub(crate) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Pane width every fixture renders at.
pub(crate) const WIDTH: u16 = 80;
/// Frame height every fixture renders at: taller than any fixture's entry.
pub(crate) const HEIGHT: u16 = 24;

// region:    --- Theme

/// The theme under test: distinct colors, so every assertion names one surface
/// of the contract.
pub(crate) fn test_theme() -> Theme {
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
        markdown: agnt_render::Markdown {
            code_keyword: Color::rgb(40, 40, 40),
            ..agnt_render::Markdown::default()
        },
    }
}

/// A second theme with every surface moved, so a caller-provided theme cannot
/// be confused with the default one.
pub(crate) fn alt_theme() -> Theme {
    Theme {
        base: Color::rgb(101, 101, 101),
        text: Color::rgb(102, 102, 102),
        muted: Color::rgb(103, 103, 103),
        idle: Color::rgb(109, 109, 109),
        accent: Color::rgb(104, 104, 104),
        user_accent: Color::rgb(111, 111, 111),
        running: Color::rgb(112, 112, 112),
        success: Color::rgb(107, 107, 107),
        error: Color::rgb(108, 108, 108),
        user_surface: Color::rgb(110, 110, 110),
        code_surface: Color::rgb(120, 120, 120),
        notice: Color::rgb(105, 105, 105),
        banner: Color::rgb(106, 106, 106),
        diff_add_bg: Color::rgb(130, 130, 130),
        diff_remove_bg: Color::rgb(131, 131, 131),
        ..Theme::default()
    }
}

/// A backend-neutral color as the terminal color the backend paints.
pub(crate) fn terminal(color: Color) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(color.r, color.g, color.b)
}

// endregion: --- Theme

// region:    --- Paint

/// A backend with a blank frame at the fixture size.
pub(crate) fn blank_backend() -> RatatuiBackend {
    RatatuiBackend::new(WIDTH, HEIGHT)
}

/// Render one entry at `row` through `theme` and return the backend plus the
/// row count the backend measured.
pub(crate) fn render_at_with(
    entry: &TranscriptEntry,
    row: usize,
    theme: &Theme,
) -> Result<(RatatuiBackend, usize)> {
    let mut backend = blank_backend();
    let width = usize::from(WIDTH);
    let rows = backend.measure(entry, width);
    backend.render(entry, RowRect::new(row, width, rows), theme);
    Ok((backend, rows))
}

/// Render one entry at row 0 through the fixture theme.
pub(crate) fn render_at(entry: &TranscriptEntry, row: usize) -> Result<(RatatuiBackend, usize)> {
    render_at_with(entry, row, &test_theme())
}

/// Render one entry at row 0 and return the backend alone.
pub(crate) fn render(entry: &TranscriptEntry) -> Result<RatatuiBackend> {
    Ok(render_at(entry, 0)?.0)
}

/// Rows the backend measured for `entry` at the fixture width.
pub(crate) fn measured_rows(entry: &TranscriptEntry) -> Result<usize> {
    Ok(RatatuiBackend::new(0, 0).measure(entry, usize::from(WIDTH)))
}

/// Render one entry at row 0 into a pane `width` columns wide.
pub(crate) fn render_in(entry: &TranscriptEntry, width: u16) -> Result<(RatatuiBackend, usize)> {
    let mut backend = RatatuiBackend::new(width, HEIGHT);
    let rows = backend.measure(entry, usize::from(width));
    backend.render(
        entry,
        RowRect::new(0, usize::from(width), rows),
        &test_theme(),
    );
    Ok((backend, rows))
}

// endregion: --- Paint

// region:    --- Cell reads

/// The symbols of one row, left to right.
pub(crate) fn row_text(backend: &RatatuiBackend, y: u16) -> String {
    (0..WIDTH)
        .map(|x| backend.buffer()[(x, y)].symbol().to_string())
        .collect()
}

/// The trimmed symbols of one row.
pub(crate) fn row_trimmed(backend: &RatatuiBackend, y: u16) -> String {
    row_text(backend, y).trim_end().to_string()
}

/// The background of one cell.
pub(crate) fn cell_bg(backend: &RatatuiBackend, x: u16, y: u16) -> Option<ratatui::style::Color> {
    backend.buffer()[(x, y)].style().bg
}

/// The foreground of one cell.
pub(crate) fn cell_fg(backend: &RatatuiBackend, x: u16, y: u16) -> Option<ratatui::style::Color> {
    backend.buffer()[(x, y)].style().fg
}

/// Whether a cell carries no background. An unpainted cell reports the reset
/// color, so "untinted" covers both the absent and the reset state.
pub(crate) fn is_untinted(bg: Option<ratatui::style::Color>) -> bool {
    matches!(bg, None | Some(ratatui::style::Color::Reset))
}

/// Number of leading rows the entry painted: a row counts while any cell of it
/// carries a glyph or a background.
pub(crate) fn painted_rows(backend: &RatatuiBackend) -> usize {
    let mut count = 0;
    for y in 0..HEIGHT {
        let painted = (0..WIDTH).any(|x| {
            let cell = &backend.buffer()[(x, y)];
            cell.symbol() != " " || !is_untinted(cell.style().bg)
        });
        if !painted {
            break;
        }
        count += 1;
    }
    count
}

/// The first column of `row` whose symbol is not a space.
pub(crate) fn first_glyph_column(backend: &RatatuiBackend, row: u16) -> Option<u16> {
    (0..WIDTH).find(|x| backend.buffer()[(*x, row)].symbol() != " ")
}

/// The symbols of one row of a backend that is `width` columns wide.
pub(crate) fn row_text_in(backend: &RatatuiBackend, width: u16, y: u16) -> String {
    (0..width)
        .map(|x| backend.buffer()[(x, y)].symbol().to_string())
        .collect()
}

/// The first row whose symbols contain `needle`.
pub(crate) fn row_index_containing(backend: &RatatuiBackend, needle: &str) -> Option<u16> {
    (0..HEIGHT).find(|y| row_text(backend, *y).contains(needle))
}

// endregion: --- Cell reads

// region:    --- Fixtures

/// Prose long enough to wrap at [`WIDTH`]: 30 eight-character words.
pub(crate) fn long_prose() -> String {
    (0..30)
        .map(|index| format!("word{index:05}"))
        .collect::<Vec<String>>()
        .join(" ")
}

pub(crate) fn message_entry(role: MessageRole, markdown: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role,
        markdown: markdown.to_string(),
    })
}

/// A tool entry. `preview` and `result` are the two display slots; the widget
/// renders their unified slot, `result.or(preview)`.
pub(crate) fn tool_entry(
    name: &str,
    summary: &str,
    preview: Option<Display>,
    result: Option<Display>,
    status: ItemStatus,
) -> TranscriptEntry {
    TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::from_name(&ToolName(name.to_string())),
        call: ToolCall::new(
            ToolName(name.to_string()),
            "",
            "",
            CallLine {
                summary: summary.to_string(),
            },
        ),
        preview,
        result,
        status,
    })
}

/// A bare tool call: no display body.
pub(crate) fn bare_tool(status: ItemStatus) -> TranscriptEntry {
    tool_entry("read", "→ a.rs", None, None, status)
}

pub(crate) fn notice_entry(kind: NoticeKind, text: &str) -> TranscriptEntry {
    TranscriptEntry::Notice(NoticeEntry {
        kind,
        text: text.to_string(),
    })
}

pub(crate) fn banner_entry(text: &str) -> TranscriptEntry {
    TranscriptEntry::Banner(BannerEntry {
        text: text.to_string(),
    })
}

pub(crate) fn plain_display(content: &str) -> Display {
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

pub(crate) fn code_display(language: &str, content: &str) -> Display {
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

/// A titled, labelled code display: one title row and one label row above the
/// code body, all on the code surface.
pub(crate) fn titled_code_display(title: &str, content: &str) -> Display {
    Display {
        title: Some(title.to_string()),
        sections: vec![DisplaySection {
            label: Some(title.to_string()),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: content.to_string(),
            stats: None,
        }],
    }
}

pub(crate) fn diff_display(content: &str) -> Display {
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

/// A one-hunk diff: file headers, a hunk header, a removed and an added line.
pub(crate) const SIMPLE_DIFF: &str = "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n";

// endregion: --- Fixtures
