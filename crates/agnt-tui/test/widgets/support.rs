//! Shared fixtures for the widget suite.

use agnt_render::transcript::entry::{
    BannerEntry, MessageEntry, NoticeEntry, TaskEntry, ToolEntry, ToolKind, TranscriptEntry,
};
use agnt_render::transcript::ir::{
    ContentKind, Display, DisplaySection, DisplayStats, MessageRole, NoticeKind, ToolName,
};
use agnt_render::transcript::renderer::ItemStatus;
use agnt_render::{Color, RenderBackend, RowRect, Theme};
use agnt_runtime::protocol::events::{PermissionRecord, RequestId, ToolCall, ToolOutcome};
use agnt_runtime::session::tool_status::ToolStatus;

use crate::state::AppState;
use crate::widgets::RatatuiBackend;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Pane width every fixture renders at.
pub(super) const WIDTH: u16 = 40;
/// Frame height every fixture renders at.
pub(super) const HEIGHT: u16 = 16;
/// The arguments of the edit call every edit fixture makes.
const EDIT_ARGUMENTS: &str = r#"{"path":"a.rs"}"#;

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

/// The background the row containing `needle` paints at column 0. The capture
/// must carry that row, so a renamed sample fails loudly.
pub(super) fn row_background(
    backend: &RatatuiBackend,
    count: usize,
    needle: &str,
) -> Result<Option<ratatui::style::Color>> {
    let last = u16::try_from(count)?;
    let found = (0..last).find(|y| row_text(backend, *y).contains(needle));
    let Some(y) = found else {
        return Err(format!("the capture must render {needle:?}").into());
    };
    Ok(cell_bg(backend, 0, y))
}

// endregion: --- Cell reads

// region:    --- Fixtures

pub(super) fn message_entry(role: MessageRole, markdown: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role,
        markdown: markdown.to_string(),
        timestamp_millis: None,
        model: None,
        agent: None,
    })
}

/// A prose entry carrying the footer facts its producer holds: the stamp and
/// the model identity, or nothing.
pub(super) fn stamped_message_entry(
    role: MessageRole,
    markdown: &str,
    timestamp_millis: Option<i64>,
    model: Option<&str>,
) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role,
        markdown: markdown.to_string(),
        timestamp_millis,
        model: model.map(str::to_string),
        agent: None,
    })
}

/// The stamp a footer shows for `millis`, formatted the way the widget formats
/// it: the local wall clock, minute resolution.
pub(super) fn stamp_text(millis: i64) -> String {
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|stamp| {
            stamp
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
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
            agnt_runtime::tools::handler::ToolSource::Unknown,
            r#"{"path":"a.rs"}"#,
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
            stats: Some(DisplayStats {
                files_changed: Some(1),
                insertions: Some(2),
                deletions: Some(0),
                ..DisplayStats::default()
            }),
        }],
    }
}

/// The tool call an `edit` makes: the one path it will write.
pub(super) fn edit_call() -> ToolCall {
    ToolCall::new(
        ToolName("edit".to_string()),
        agnt_runtime::tools::handler::ToolSource::Builtin,
        EDIT_ARGUMENTS,
    )
}

/// A gate request for one `edit`, carrying the diff the producer computed.
pub(super) fn edit_gate(diff: &str) -> PermissionRecord {
    PermissionRecord::Requested {
        request_id: RequestId::mint(),
        call_id: edit_call().id().clone(),
        tool: "edit".to_string(),
        source: "builtin".to_string(),
        mode: Some("apply".to_string()),
        rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ a.rs".to_string(),
        shows_preview: true,
        preview: Some(diff.to_string()),
    }
}

/// The diff every edit capture renders.
pub(super) const EDIT_DIFF: &str = "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n";

/// The backgrounds an edit's diff rows paint inside the band: the file headers
/// and the hunk sit on the code surface, the changed rows on their tints.
pub(super) fn edit_row_backgrounds() -> [(&'static str, Option<ratatui::style::Color>); 5] {
    let theme = test_theme();
    [
        ("--- a/a.rs", Some(terminal(theme.code_surface))),
        ("+++ b/a.rs", Some(terminal(theme.code_surface))),
        ("@@ -1 +1 @@", Some(terminal(theme.code_surface))),
        ("│old", Some(terminal(theme.diff_remove_bg))),
        ("│new", Some(terminal(theme.diff_add_bg))),
    ]
}

/// The backgrounds an edit capture paints, in the expectation table's order.
pub(super) fn edit_capture_backgrounds(
    backend: &RatatuiBackend,
    count: usize,
) -> Result<Vec<Option<ratatui::style::Color>>> {
    edit_row_backgrounds()
        .iter()
        .map(|(needle, _)| row_background(backend, count, needle))
        .collect()
}

/// The `edit` outcome's own JSON payload text for `diff`.
pub(super) fn edit_result_text(diff: &str) -> String {
    let escaped = diff
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!(
        "{{\"path\":\"a.rs\",\"diff\":\"{escaped}\",\"stats\":{{\"files_changed\":1,\"insertions\":1,\"deletions\":1}}}}"
    )
}

/// The outcome the `edit` handler settles on: its own JSON payload as the
/// result text.
pub(super) fn edit_outcome(diff: &str) -> ToolOutcome {
    ToolOutcome {
        status: ToolStatus::Success,
        payload: None,
        text: edit_result_text(diff),
        message: None,
        kind: None,
    }
}

/// The one entry a fixture store holds.
pub(super) fn only_entry(store: &AppState) -> Result<&TranscriptEntry> {
    store
        .transcript
        .entries()
        .first()
        .ok_or_else(|| "the store must hold one entry".into())
}

/// A task call line: the name, the subject it works on, and its status.
pub(super) fn task_entry(summary: &str, status: ItemStatus) -> TranscriptEntry {
    TranscriptEntry::Task(TaskEntry {
        name: "Compaction".to_string(),
        summary: summary.to_string(),
        status,
    })
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
