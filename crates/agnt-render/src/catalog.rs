//! The component catalog contract: the names, props, and semantics every
//! backend implements in its own style vocabulary.
//!
//! The contract carries data only. A backend composes these components into its
//! family widgets and styles them itself; the contract fixes what each
//! component means so the backends cannot drift apart on structure.

use agnt_core::protocol::event::ToolDisplayStats;
use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::{ContentLine, DiffTint, MessageRole, NoticeKind, Span};
use agnt_core::transcript::renderer::ItemStatus;

// region:    --- Types

/// The lane a transcript entry renders in: the gutter glyphs and the columns
/// they reserve ahead of the entry's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneKind {
    User,
    Tool,
    Compaction,
    System,
    Assistant,
    Banner,
}

impl LaneKind {
    /// Columns the cursor slot takes ahead of the marker on row 0.
    pub const CURSOR_SLOT_WIDTH: usize = 2;

    /// The lane of an entry.
    pub fn of_entry(entry: &TranscriptEntry) -> Self {
        match entry {
            TranscriptEntry::Message(message) => Self::of_message(message.role),
            TranscriptEntry::Tool(_) => Self::Tool,
            TranscriptEntry::Notice(notice) => Self::of_notice(notice.kind),
            TranscriptEntry::Banner(_) => Self::Banner,
        }
    }

    pub fn of_message(role: MessageRole) -> Self {
        match role {
            MessageRole::User => Self::User,
            MessageRole::Assistant => Self::Assistant,
        }
    }

    pub fn of_notice(kind: NoticeKind) -> Self {
        match kind {
            NoticeKind::Compaction => Self::Compaction,
            NoticeKind::System => Self::System,
        }
    }

    /// The row-0 marker slot, two columns wide.
    pub fn marker(self) -> &'static str {
        match self {
            Self::User => "╎ ",
            Self::Tool => "⚙ ",
            Self::Compaction => "~ ",
            Self::System => "· ",
            Self::Assistant | Self::Banner => "  ",
        }
    }

    /// The continuation-row marker slot. The user rail keeps its glyph on
    /// every row so a multi-row turn holds a constant gutter; every other lane
    /// blanks the slot.
    pub fn continuation_marker(self) -> &'static str {
        match self {
            Self::User => "╎ ",
            Self::Assistant | Self::Banner => "  ",
            Self::Tool | Self::Compaction | Self::System => "  ",
        }
    }

    /// Whether rows reserve a cursor slot ahead of the marker. The tool,
    /// notice, and banner lanes do; the user rail and prose lanes do not.
    pub fn cursor_slot(self) -> bool {
        match self {
            Self::User | Self::Assistant => false,
            Self::Tool | Self::Compaction | Self::System | Self::Banner => true,
        }
    }

    /// Columns the lane reserves before the entry's content.
    pub fn prefix_width(self) -> usize {
        let slot = if self.cursor_slot() {
            Self::CURSOR_SLOT_WIDTH
        } else {
            0
        };
        self.marker().chars().count() + slot
    }
}

/// One lane gutter line: the marker slot and, on the row the cursor marks, the
/// cursor slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneLine {
    pub kind: LaneKind,
    pub cursor: bool,
}

impl LaneLine {
    pub const fn new(kind: LaneKind) -> Self {
        Self {
            kind,
            cursor: false,
        }
    }

    /// Mark row 0 with the cursor slot.
    pub const fn with_cursor(self) -> Self {
        Self {
            kind: self.kind,
            cursor: true,
        }
    }

    /// Columns the line reserves before content.
    pub fn prefix_width(&self) -> usize {
        self.kind.prefix_width()
    }

    /// The gutter for a row. `continuation` selects the continuation marker
    /// slot and always blanks the cursor slot.
    pub fn prefix(&self, continuation: bool) -> String {
        let mut prefix = String::new();
        if self.kind.cursor_slot() {
            prefix.push_str(if self.cursor && !continuation {
                "> "
            } else {
                "  "
            });
        }
        prefix.push_str(if continuation {
            self.kind.continuation_marker()
        } else {
            self.kind.marker()
        });
        prefix
    }
}

/// A tool call's header row: the status indicator, the tool name, and the call
/// summary. The row carries no background — the surfaces below it do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallLineRow {
    pub status: Option<ItemStatus>,
    pub name: String,
    pub summary: String,
}

impl CallLineRow {
    pub fn new(
        name: impl Into<String>,
        summary: impl Into<String>,
        status: Option<ItemStatus>,
    ) -> Self {
        Self {
            status,
            name: name.into(),
            summary: summary.into(),
        }
    }

    /// The row's content spans: the name in emphasis and the summary in muted.
    /// An empty name renders the summary alone; an empty summary renders the
    /// name alone.
    pub fn spans(&self) -> Vec<Span> {
        if self.name.is_empty() {
            return vec![Span::muted(self.summary.clone())];
        }
        let mut spans = vec![Span::emphasis(self.name.clone())];
        if !self.summary.is_empty() {
            spans.push(Span::muted(format!(" {}", self.summary)));
        }
        spans
    }

    /// The status indicator glyph at a clock reading, if the call carries a
    /// status.
    pub fn indicator(&self, now_millis: u128) -> Option<&'static str> {
        self.status.map(|status| status.indicator_char(now_millis))
    }

    /// Plain text of the row: the indicator (when any) then the content spans.
    pub fn plain_text(&self, now_millis: u128) -> String {
        let content: String = self.spans().iter().map(|span| span.text.as_str()).collect();
        match self.indicator(now_millis) {
            Some(indicator) => format!("{indicator} {content}"),
            None => content,
        }
    }
}

/// A display section's stats row: the display's own formatted stats line in
/// muted, and no row when the stats carry no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatRow {
    pub stats: ToolDisplayStats,
}

impl StatRow {
    pub fn new(stats: ToolDisplayStats) -> Self {
        Self { stats }
    }

    pub fn text(&self) -> Option<String> {
        self.stats.format_stats_line()
    }

    pub fn spans(&self) -> Vec<Span> {
        self.text()
            .map(|text| vec![Span::muted(text)])
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.text().is_none()
    }
}

/// A diff body: the projected diff rows. Each row carries its own line-number
/// gutter in its spans and its own line-level tint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffView {
    pub lines: Vec<ContentLine>,
}

impl DiffView {
    pub fn new(lines: Vec<ContentLine>) -> Self {
        Self { lines }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// The line-level tint a row paints over the surrounding surface.
    pub fn tint(&self, row: usize) -> Option<&DiffTint> {
        self.lines.get(row).and_then(|line| line.diff_tint.as_ref())
    }
}

/// A code or diff surface: the rows that paint on the code surface, with one
/// padding row above and below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSurface {
    pub rows: Vec<SurfaceRow>,
}

/// One surface row. A code section contributes text rows; a diff section
/// contributes a [`DiffView`] whose row tints paint over the surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceRow {
    Text(ContentLine),
    Diff(DiffView),
}

impl CodeSurface {
    /// One padding row above and one below the surface's rows.
    pub const MARGIN_ROWS: usize = 2;

    pub fn new(rows: Vec<SurfaceRow>) -> Self {
        Self { rows }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// One catalog component value. The enum is the static-dispatch bundle: a
/// heterogeneous component list without trait objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogComponent {
    LaneLine(LaneLine),
    CallLineRow(CallLineRow),
    StatRow(StatRow),
    DiffView(DiffView),
    CodeSurface(CodeSurface),
}

impl CatalogComponent {
    /// Stable kebab-case component name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::LaneLine(_) => "lane-line",
            Self::CallLineRow(_) => "call-line-row",
            Self::StatRow(_) => "stat-row",
            Self::DiffView(_) => "diff-view",
            Self::CodeSurface(_) => "code-surface",
        }
    }
}

// endregion: --- Types

// region:    --- Tests

#[cfg(test)]
#[path = "../test/catalog.rs"]
mod catalog_test;

// endregion: --- Tests
