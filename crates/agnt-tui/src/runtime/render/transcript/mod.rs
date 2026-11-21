//! The transcript pane: the frame plan painted through the ratatui backend.
//!
//! The pane owns the two things no entry widget can know: the scroll window and
//! the gap rows between neighbours. It asks the store for the frame plan of the
//! visible window, paints each planned entry through [`RatatuiBackend`], and
//! paints the gap rows itself — a gap row bleeds the user surface when a user
//! turn borders it, which is a fact about two neighbours, not about one entry.
//! The pane also owns the cursor slot and the selection overlay: the backend
//! paints entry content only.
//!
//! One entry's own rows — its canvas, the rows the window copies out of it, and
//! the cell whose glyph names the frame clock — live in [`entry_paint`].

// region:    --- Modules

mod entry_paint;

use entry_paint::{animated_indicator_cell, paint_entry_rows};

// endregion: --- Modules

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use agnt_render::Theme;
use agnt_render::planner::PlannedEntry;
use agnt_render::transcript::entry::TranscriptEntry;
use agnt_render::transcript::ir::MessageRole;

use crate::rendering::theme::TuiTheme;
use crate::runtime::render::clock_cells::ClockCell;
use crate::runtime::render::paint_user_rail_row;
use crate::runtime::render::row_cache::RowCache;
use crate::state::{InputMode, PaneFocus, TranscriptStore};
use crate::widgets::PaintCtx;

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
    ) -> Vec<ClockCell> {
        frame.render_widget(Clear, transcript_content_area);
        frame.render_widget(
            Block::default().style(Style::default().bg(self.theme.base)),
            transcript_content_area,
        );
        if transcript_content_area.height == 0 {
            return Vec::new();
        }
        let width = transcript_list_area.width as usize;
        let viewport_height = transcript_list_area.height as usize;
        if width == 0 || viewport_height == 0 {
            return Vec::new();
        }

        // Settle the height index before anything reads it: a wholesale rebuild
        // when the width changed, and the one re-measure a streamed fold left
        // pending — so a burst of chunks costs one measurement per frame.
        self.state.transcript.ensure_height_index(width);

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
            now_millis: self.frame_clock_millis,
        };

        let mut canvas = Buffer::empty(Rect::new(
            0,
            0,
            u16::try_from(width).unwrap_or(u16::MAX),
            u16::try_from(viewport_height).unwrap_or(u16::MAX),
        ));
        let clock_cells = paint_transcript_rows(
            &self.state.transcript,
            &window,
            &mut self.row_cache,
            &mut canvas,
        );
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
                    .thumb_symbol("┃")
                    .thumb_style(self.theme.focus)
                    .track_style(self.theme.subtle_meta),
                transcript_content_area,
                &mut scrollbar_state,
            );
        }

        clock_cells
            .into_iter()
            .map(|cell| cell.offset_by(transcript_list_area))
            .collect()
    }
}

// region:    --- Paint

/// One paint pass's frame-wide inputs: the width, the absolute row window,
/// where that window lands in the target buffer, and the frame clock the rows
/// read.
pub(crate) struct PaintWindow<'a> {
    pub theme: &'a TuiTheme,
    pub width: usize,
    /// Absolute row that lands on buffer row `top`.
    pub first_row: usize,
    /// Rows of absolute transcript content the buffer shows.
    pub height: usize,
    /// Buffer row that hosts `first_row`.
    pub top: usize,
    /// The frame clock the rows read: an in-progress indicator cycles with it.
    pub now_millis: u128,
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
///
/// `cache` carries each entry's painted rows from one frame to the next. It is
/// a memo, never a source of truth: the bytes are the same whether a frame
/// blits a parked canvas or re-renders the entry.
///
/// Returns the cells whose glyph reads the frame clock, in buffer coordinates.
pub(crate) fn paint_transcript_rows(
    store: &TranscriptStore,
    window: &PaintWindow<'_>,
    cache: &mut RowCache,
    buf: &mut Buffer,
) -> Vec<ClockCell> {
    if window.width == 0 || window.height == 0 {
        return Vec::new();
    }
    let (first, last) = store.visible_window(window.first_row, window.height);
    let plan = store.frame_plan(first, last, window.width);
    let entries = store.entries();
    let paint_theme = Theme::from(window.theme);
    let now_millis = window.now_millis;
    // A width or theme that moved since the last frame drops every parked
    // canvas, before this frame reads one.
    cache.begin_frame(window.width, paint_theme);
    let ctx = PaintCtx {
        theme: &paint_theme,
        now_millis,
    };
    let mut clock_cells: Vec<ClockCell> = Vec::new();

    for planned in &plan.entries {
        let Some(entry) = entries.get(planned.index) else {
            continue;
        };
        let previous = planned
            .index
            .checked_sub(1)
            .and_then(|index| entries.get(index));
        paint_gap_rows(buf, planned, previous, entry, window);
        paint_entry_rows(
            buf,
            planned,
            entry,
            store.entry_version(planned.index),
            window,
            ctx,
            cache,
        );
        if let Some(cell) = animated_indicator_cell(planned, entry, window, &paint_theme) {
            clock_cells.push(cell);
        }
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

    clock_cells
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
