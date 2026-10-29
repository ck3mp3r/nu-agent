//! The picker footer.
//!
//! One component owns the selected picker value's footer: the wrap, the cap, the
//! rect height, and the text placement. A picker arm supplies the text, the
//! column its surface's text starts at, the footer width, and the ladder's
//! reserved budget; the component supplies everything else.
//!
//! Contract:
//! - The wrap is a greedy first-fit on ASCII spaces, capped at `reserved` rows,
//!   and breaks `text_indent + 1` cells short of the footer width so one blank
//!   cell stays against the right border.
//! - `height()` is the row count the caller puts in the split's last constraint,
//!   so the footer's last row is the inner area's bottom row by construction. The
//!   count carries one gap row above the text, so a list that fills its area
//!   never touches the footer.
//! - `render` paints foreground-only text at `rect.x + text_indent` below the gap
//!   row: no background band, no pad rows, no bold. Every surveyed TUI renders
//!   in-popup detail text foreground-only (lazygit's list footer, fzf's info
//!   line), so the footer reads as one more text row on the popup base and its
//!   first glyph lands on the surface's own text column.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Text},
    widgets::Paragraph,
};

use crate::{rendering::theme::TuiTheme, runtime::panels::wrap_at_ascii_spaces};

/// The blank rows the footer claims between the table area and the text.
const GAP_ROWS: u16 = 1;

// region:    --- Types

/// The selected picker value's footer: the wrapped rows it paints and the column
/// its surface's text starts at. Empty when there is no text or no reserved
/// budget.
#[derive(Default)]
pub(crate) struct PickerFooter {
    lines: Vec<Line<'static>>,
    text_indent: u16,
}

// endregion: --- Types

// region:    --- Support

impl PickerFooter {
    // region:    --- Constructors

    /// Measures the footer for `text` at the picker's inner `width` display
    /// columns, capped at the ladder's `reserved` budget. The text wraps at
    /// `width - text_indent - 1`, so it ends one cell short of the right border.
    pub(crate) fn measure(text: Option<&str>, text_indent: u16, width: u16, reserved: u16) -> Self {
        let Some(text) = text else {
            return Self::default();
        };
        if reserved == 0 {
            return Self::default();
        }
        let wrap_width = width.saturating_sub(text_indent.saturating_add(1));
        let mut lines = wrap_at_ascii_spaces(text, wrap_width);
        lines.truncate(usize::from(reserved));
        Self {
            lines: lines.into_iter().map(Line::from).collect(),
            text_indent,
        }
    }

    // endregion: --- Constructors

    // region:    --- Accessors

    /// The rows the footer claims: the split's last constraint, which bottom-pins
    /// the text against the inner area's last row. The gap row sits above the
    /// text, so an empty footer still claims no rows.
    pub(crate) fn height(&self) -> u16 {
        if self.lines.is_empty() {
            return 0;
        }
        u16::try_from(self.lines.len())
            .unwrap_or(u16::MAX)
            .saturating_add(GAP_ROWS)
    }

    // endregion: --- Accessors

    // region:    --- Commands

    /// Paints the wrapped text over `rect` below its gap row, starting
    /// `text_indent` cells in from the rect's left edge, in the popup's meta
    /// foreground with no background and no bold. The gap row stays unpainted, so
    /// the caller's base background shows through. A footer with no rows renders
    /// nothing.
    pub(crate) fn render(&self, frame: &mut Frame, rect: Rect, theme: &TuiTheme) {
        if self.lines.is_empty() || rect.height <= GAP_ROWS {
            return;
        }
        let text_rect = Rect {
            x: rect.x.saturating_add(self.text_indent),
            y: rect.y.saturating_add(GAP_ROWS),
            width: rect.width.saturating_sub(self.text_indent),
            height: rect.height.saturating_sub(GAP_ROWS),
        };
        frame.render_widget(
            Paragraph::new(Text::from(self.lines.clone())).style(theme.subtle_meta),
            text_rect,
        );
    }

    // endregion: --- Commands
}

// endregion: --- Support
