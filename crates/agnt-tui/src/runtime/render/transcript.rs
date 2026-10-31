//! The transcript pane: the frame plan painted through the ratatui backend.
//!
//! The pane owns the two things no entry widget can know: the scroll window and
//! the gap rows between neighbours. It asks the store for the frame plan of the
//! visible window, paints each planned entry through [`RatatuiBackend`], and
//! paints the gap rows itself — a gap row bleeds the user surface when a user
//! turn borders it, which is a fact about two neighbours, not about one entry.
//! The pane also owns the cursor slot and the selection overlay: the backend
//! paints entry content only.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::MessageRole;
use agnt_render::backend::RowRect;
use agnt_render::planner::PlannedEntry;
use agnt_render::{RenderBackend, Theme};

use crate::rendering::theme::TuiTheme;
use crate::runtime::render::frame::current_time_millis;
use crate::runtime::render::paint_user_rail_row;
use crate::state::{InputMode, PaneFocus, TranscriptStore};
use crate::widgets::RatatuiBackend;

use crate::runtime::RuntimeCoordinator;

impl RuntimeCoordinator {
    pub(crate) fn render_transcript_pane(
        &mut self,
        frame: &mut ratatui::Frame,
        transcript_content_area: Rect,
        transcript_list_area: Rect,
        transcript_following_tail: bool,
        transcript_scroll_offset: usize,
        rendered_scroll_offset: &mut Option<usize>,
    ) {
        frame.render_widget(Clear, transcript_content_area);
        frame.render_widget(
            Block::default().style(Style::default().bg(self.theme.base)),
            transcript_content_area,
        );
        if transcript_content_area.height == 0 {
            return;
        }
        let width = transcript_list_area.width as usize;
        let viewport_height = transcript_list_area.height as usize;
        if width == 0 || viewport_height == 0 {
            return;
        }

        // Rebuild the height index when stale (entries added, evicted, or resize)
        if !self.state.transcript.height_index_valid_for(width) {
            self.state.transcript.rebuild_height_index(width);
        }

        let total_visual_rows = self.state.transcript.total_visual_rows();
        self.state.scroll.viewport_height = viewport_height;
        let max_scroll = self.state.scroll.sync_after_render(total_visual_rows);
        let effective_offset: usize = if transcript_following_tail {
            max_scroll
        } else {
            transcript_scroll_offset.min(max_scroll)
        };
        *rendered_scroll_offset = Some(effective_offset);
        // When following tail, sync_after_render keeps the cursor at the
        // last visual row; when not tailing, cursor_visual_row is
        // user-controlled — untouched.

        // Bottom-align content shorter than the viewport: the padding splits
        // evenly above and below, so a fresh session's logo sits just above
        // the input box instead of at the top of an empty pane.
        let top_pad = viewport_height.saturating_sub(total_visual_rows) / 2;
        let window = PaintWindow {
            theme: &self.theme,
            width,
            first_row: effective_offset,
            height: viewport_height,
            top: top_pad,
        };

        let mut canvas = Buffer::empty(Rect::new(
            0,
            0,
            u16::try_from(width).unwrap_or(u16::MAX),
            u16::try_from(viewport_height).unwrap_or(u16::MAX),
        ));
        paint_transcript_rows(&self.state.transcript, &window, &mut canvas);
        blit(frame.buffer_mut(), &canvas, transcript_list_area);

        // Post-render buffer manipulation: apply selection highlight to visual rows
        if self.state.input.mode == InputMode::Visual
            && let Some(sel) = &self.state.scroll.selection
        {
            let (sel_start, sel_end) = sel.normalized_range();
            Self::apply_selection_highlight(
                frame.buffer_mut(),
                transcript_list_area,
                sel_start,
                sel_end,
                &window,
                self.theme.selection_bg,
            );
        }

        // Overlay > cursor indicator at the correct screen position. The
        // backend paints the cursor slot blank, so the pane owns it. The row
        // comes from the same [`PaintWindow`] the paint used, so the marker
        // rides the window's `top` padding with its own content row.
        if self.state.scroll.pane_focus == PaneFocus::Transcript
            && (self.state.input.mode == InputMode::Normal
                || self.state.input.mode == InputMode::Visual)
            && window.contains(self.state.scroll.cursor_visual_row)
            && let Ok(buffer_row) =
                u16::try_from(window.buffer_row(self.state.scroll.cursor_visual_row))
        {
            let cursor_screen_y = transcript_list_area.y.saturating_add(buffer_row);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("> ", self.theme.focus))),
                Rect::new(transcript_list_area.x, cursor_screen_y, 2, 1),
            );
        }

        if total_visual_rows > viewport_height {
            let mut scrollbar_state = ScrollbarState::new(max_scroll).position(effective_offset);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .thumb_style(self.theme.focus)
                    .track_style(self.theme.subtle_meta),
                transcript_content_area,
                &mut scrollbar_state,
            );
        }
    }
}

// region:    --- Paint

/// One paint pass's frame-wide inputs: the width, the absolute row window, and
/// where that window lands in the target buffer.
pub(crate) struct PaintWindow<'a> {
    pub theme: &'a TuiTheme,
    pub width: usize,
    /// Absolute row that lands on buffer row `top`.
    pub first_row: usize,
    /// Rows of absolute transcript content the buffer shows.
    pub height: usize,
    /// Buffer row that hosts `first_row`.
    pub top: usize,
}

impl PaintWindow<'_> {
    /// Whether the absolute row `row` falls inside the shown window.
    pub(crate) fn contains(&self, row: usize) -> bool {
        row >= self.first_row && row < self.first_row.saturating_add(self.height)
    }

    /// The buffer row that hosts the absolute row `row`.
    pub(crate) fn buffer_row(&self, row: usize) -> usize {
        self.top + row.saturating_sub(self.first_row)
    }
}

/// Paint the transcript window into `buf`.
///
/// Pure apart from the frame clock: no scroll state, no terminal. The pane and
/// the yank path both render through it, so the visible frame and the copied
/// text cannot drift apart.
pub(crate) fn paint_transcript_rows(
    store: &TranscriptStore,
    window: &PaintWindow<'_>,
    buf: &mut Buffer,
) {
    if window.width == 0 || window.height == 0 {
        return;
    }
    let (first, last) = store.visible_window(window.first_row, window.height);
    let plan = store.frame_plan(first, last, window.width);
    let entries = store.entries();
    let paint_theme = Theme::from(window.theme);
    let now_millis = current_time_millis();

    for planned in &plan.entries {
        let Some(entry) = entries.get(planned.index) else {
            continue;
        };
        let previous = planned
            .index
            .checked_sub(1)
            .and_then(|index| entries.get(index));
        paint_gap_rows(buf, planned, previous, entry, window);
        paint_entry_rows(buf, planned, entry, window, &paint_theme, now_millis);
    }

    if let Some(planned) = plan.entries.last() {
        let trailing_start = planned.rect.end();
        let trailing_bleed = entries.get(planned.index).is_some_and(is_user_turn);
        for row in trailing_start..trailing_start.saturating_add(plan.trailing_gap) {
            if !window.contains(row) {
                continue;
            }
            paint_gap_row(
                buf,
                window.buffer_row(row),
                window.width,
                window.theme,
                trailing_bleed,
            );
        }
    }
}

/// Paint the separator rows before one planned entry. The first row of a run
/// bleeds the user surface when the previous entry is a user turn; the last
/// row bleeds when the incoming entry is one. A bleeding row carries the user
/// rail so the box's top and bottom rows keep the gutter.
fn paint_gap_rows(
    buf: &mut Buffer,
    planned: &PlannedEntry,
    previous: Option<&TranscriptEntry>,
    entry: &TranscriptEntry,
    window: &PaintWindow<'_>,
) {
    if planned.gap_before == 0 {
        return;
    }
    // A planned rect starts at the entry's top padding row, so its gap rows
    // sit directly above it.
    let gap_end = planned.rect.row;
    let gap_start = gap_end.saturating_sub(planned.gap_before);
    let previous_is_user = previous.is_some_and(is_user_turn);
    let incoming_is_user = is_user_turn(entry);
    for row in gap_start..gap_end {
        if !window.contains(row) {
            continue;
        }
        let bleed =
            (row == gap_start && previous_is_user) || (row + 1 == gap_end && incoming_is_user);
        paint_gap_row(
            buf,
            window.buffer_row(row),
            window.width,
            window.theme,
            bleed,
        );
    }
}

/// Paint one separator row: the row background, and the user rail when the row
/// sits inside the user box.
fn paint_gap_row(buf: &mut Buffer, y: usize, width: usize, theme: &TuiTheme, bleed: bool) {
    let Ok(y) = u16::try_from(y) else {
        return;
    };
    let bg = if bleed { theme.row_user_bg } else { theme.base };
    let width = u16::try_from(width).unwrap_or(u16::MAX).min(buf.area.width);
    for x in buf.area.left()..width {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_bg(bg);
        }
    }
    if bleed {
        paint_user_rail_row(buf, y, usize::from(width), theme);
    }
}

/// Paint one planned entry's visible rows. The entry renders into a backend
/// sized to its own row rect at row 0, so a partially scrolled entry copies the
/// rows the window shows rather than the rows above it.
fn paint_entry_rows(
    buf: &mut Buffer,
    planned: &PlannedEntry,
    entry: &TranscriptEntry,
    window: &PaintWindow<'_>,
    theme: &Theme,
    now_millis: u128,
) {
    if planned.rect.rows == 0 {
        return;
    }
    let visible_start = planned.rect.row.max(window.first_row);
    let visible_end = planned
        .rect
        .end()
        .min(window.first_row.saturating_add(window.height));
    if visible_start >= visible_end {
        return;
    }
    let Ok(rows) = u16::try_from(planned.rect.rows) else {
        return;
    };
    let mut backend = RatatuiBackend::new(u16::try_from(window.width).unwrap_or(u16::MAX), rows);
    backend.set_now_millis(now_millis);
    backend.render(
        entry,
        RowRect::new(0, window.width, planned.rect.rows)
            .with_content_offset(planned.rect.content_offset),
        theme,
    );

    let canvas = backend.buffer();
    for row in visible_start..visible_end {
        let Ok(source_y) = u16::try_from(row - planned.rect.row) else {
            continue;
        };
        let Ok(target_y) = u16::try_from(window.buffer_row(row)) else {
            continue;
        };
        for x in 0..canvas.area.width {
            let Some(cell) = canvas.cell((x, source_y)) else {
                continue;
            };
            if *cell == Cell::EMPTY {
                continue;
            }
            if let Some(target) = buf.cell_mut((x, target_y)) {
                *target = cell.clone();
            }
        }
    }
}

/// Copy `canvas` into `target` at `area`. Only painted cells travel: an
/// untouched canvas cell must not erase the pane's background.
///
/// A widget paints content, not the pane's base surface: a cell whose
/// background is reset leaves the target's background in place, so the pane's
/// base fill survives under every lane gutter and every prose glyph.
pub(super) fn blit(target: &mut Buffer, canvas: &Buffer, area: Rect) {
    for y in 0..canvas.area.height {
        for x in 0..canvas.area.width {
            let Some(cell) = canvas.cell((x, y)) else {
                continue;
            };
            if *cell == Cell::EMPTY {
                continue;
            }
            let position = Position {
                x: area.x.saturating_add(x),
                y: area.y.saturating_add(y),
            };
            let Some(slot) = target.cell_mut(position) else {
                continue;
            };
            let background = slot.bg;
            *slot = cell.clone();
            if cell.bg == Color::Reset {
                slot.set_bg(background);
            }
        }
    }
}

/// Whether an entry is a user turn. The gap rows around a user turn bleed the
/// user surface, so the box reads as one block.
fn is_user_turn(entry: &TranscriptEntry) -> bool {
    matches!(
        entry,
        TranscriptEntry::Message(message) if message.role == MessageRole::User
    )
}

// endregion: --- Paint
