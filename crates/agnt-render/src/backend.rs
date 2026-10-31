//! The backend seam: one trait, one rect.

use agnt_core::transcript::entry::TranscriptEntry;

use crate::theme::Theme;

// region:    --- Types

/// Where one entry lands in the frame: its first row, its width, and its row
/// count. Rows are absolute frame rows, so a backend never has to accumulate
/// offsets itself.
///
/// `rows` counts the entry's content plus the padding the planner keeps inside
/// this rect. `content_offset` is where the content starts: zero when the
/// entry is flush or its top padding is clipped at the frame edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RowRect {
    pub row: usize,
    pub width: usize,
    pub rows: usize,
    pub content_offset: usize,
}

impl RowRect {
    pub const fn new(row: usize, width: usize, rows: usize) -> Self {
        Self {
            row,
            width,
            rows,
            content_offset: 0,
        }
    }

    /// Keep `content_offset` blank padding rows above the entry's content.
    pub const fn with_content_offset(mut self, content_offset: usize) -> Self {
        self.content_offset = content_offset;
        self
    }

    /// The first row past this rect.
    pub const fn end(&self) -> usize {
        self.row + self.rows
    }

    pub const fn is_empty(&self) -> bool {
        self.rows == 0
    }
}

/// One rendering backend. The entry type carries the family, so the backend's
/// own match on it owns the whole rendering for that family: a new family is a
/// new match arm, never a new trait method.
///
/// [`RenderBackend::measure`] must return the number of content rows
/// [`RenderBackend::render`] projects for the same entry and width. The
/// planner adds the entry's own padding rows to that count and hands the
/// result back as the rect, so `render` writes exactly `rect.rows` rows: the
/// padding blanks around the content, which starts at `rect.content_offset`.
/// [`crate::LayoutPlanner`] plans with `measure`, so a drift between the two
/// puts the frame plan and the painted output out of step.
pub trait RenderBackend {
    /// Render one entry into the backend's own output at `rect`.
    fn render(&mut self, entry: &TranscriptEntry, rect: RowRect, theme: &Theme);

    /// Number of content rows `entry` renders to at `width`, excluding the
    /// entry's padding rows.
    fn measure(&self, entry: &TranscriptEntry, width: usize) -> usize;
}

// endregion: --- Types
