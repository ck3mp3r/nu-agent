//! One planned entry's rows: the canvas it renders into, the rows the window
//! shows, and the cell whose glyph names the frame clock.
//!
//! The pane decides which entries are visible and where; this module paints one
//! of them. Its canvas comes from the row cache when the entry's content, width,
//! and theme still match its last paint, so a frame that changed nothing for the
//! entry does no projection, no wrap, and no style work for it.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Position;

use agnt_render::backend::RowRect;
use agnt_render::catalog::LaneKind;
use agnt_render::planner::PlannedEntry;
use agnt_render::transcript::entry::TranscriptEntry;
use agnt_render::transcript::renderer::ItemStatus;
use agnt_render::{RenderBackend, Theme};

use crate::runtime::render::clock_cells::{ClockCell, ClockGlyph};
use crate::runtime::render::row_cache::RowCache;
use crate::runtime::render::transcript::PaintWindow;
use crate::state::EntryVersion;
use crate::widgets::{PaintCtx, RatatuiBackend, status_style};

/// Paint one planned entry's visible rows. The entry renders into a backend
/// sized to its own row rect at row 0, so a partially scrolled entry copies the
/// rows the window shows rather than the rows above it.
///
/// The canvas comes from the cache when the entry's content, width, and theme
/// match its last paint: that frame does no projection, no wrap, and no style
/// work for the entry. A miss renders the entry once and parks the canvas.
///
/// An entry whose paint reads the frame clock is never parked: an in-progress
/// call cycles its status glyph, so its rows are not the same from one frame to
/// the next.
pub(super) fn paint_entry_rows(
    buf: &mut Buffer,
    planned: &PlannedEntry,
    entry: &TranscriptEntry,
    version: Option<EntryVersion>,
    window: &PaintWindow<'_>,
    ctx: PaintCtx<'_>,
    cache: &mut RowCache,
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
    let content_offset = planned.rect.content_offset;
    let parkable = version.filter(|_| entry_clock_status(entry).is_none());

    if let Some(version) = parkable
        && let Some(canvas) = cache.get(version, planned.rect.rows, content_offset)
    {
        copy_entry_rows(
            buf,
            canvas,
            planned.rect.row,
            visible_start,
            visible_end,
            window,
        );
        return;
    }

    let mut backend = RatatuiBackend::new(u16::try_from(window.width).unwrap_or(u16::MAX), rows);
    backend.set_now_millis(ctx.now_millis);
    backend.render(
        entry,
        RowRect::new(0, window.width, planned.rect.rows).with_content_offset(content_offset),
        ctx.theme,
    );
    if let Some(version) = parkable {
        cache.insert(
            version,
            planned.rect.rows,
            content_offset,
            backend.buffer().clone(),
        );
    }
    copy_entry_rows(
        buf,
        backend.buffer(),
        planned.rect.row,
        visible_start,
        visible_end,
        window,
    );
}

/// Copy the window's rows of one entry's canvas into the frame buffer. Only
/// painted cells travel: an untouched canvas cell must leave the pane's
/// background in place.
fn copy_entry_rows(
    buf: &mut Buffer,
    canvas: &Buffer,
    entry_first_row: usize,
    visible_start: usize,
    visible_end: usize,
    window: &PaintWindow<'_>,
) {
    for row in visible_start..visible_end {
        let Ok(source_y) = u16::try_from(row - entry_first_row) else {
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

/// The cell an animated entry's indicator occupies, or `None` when the entry
/// does not animate or its indicator row is outside the window.
///
/// An entry's call row opens its content, so the indicator sits at the entry's
/// first row, one gutter past the buffer's left edge — the columns its lane
/// reserves before the content.
pub(super) fn animated_indicator_cell(
    planned: &PlannedEntry,
    entry: &TranscriptEntry,
    window: &PaintWindow<'_>,
    paint_theme: &Theme,
) -> Option<ClockCell> {
    let status = entry_clock_status(entry)?;
    if !window.contains(planned.rect.row) {
        return None;
    }
    let x = u16::try_from(LaneKind::of_entry(entry).prefix_width()).ok()?;
    if usize::from(x) >= window.width {
        return None;
    }
    Some(ClockCell {
        position: Position {
            x,
            y: u16::try_from(window.buffer_row(planned.rect.row)).ok()?,
        },
        glyph: ClockGlyph::Item(status),
        style: status_style(status, paint_theme),
    })
}

/// The status an entry's indicator reads from the frame clock, or `None` when
/// the entry paints no clock-reading indicator.
///
/// Only an in-progress status animates: it cycles a spinner glyph, so its rows
/// are never the same twice and must not be parked, and the cell that carries
/// the glyph is what a clock-only frame repaints.
fn entry_clock_status(entry: &TranscriptEntry) -> Option<ItemStatus> {
    match entry {
        TranscriptEntry::Tool(tool) if tool.status == ItemStatus::InProgress => Some(tool.status),
        TranscriptEntry::Task(task) if task.status == ItemStatus::InProgress => Some(task.status),
        TranscriptEntry::Tool(_)
        | TranscriptEntry::Task(_)
        | TranscriptEntry::Message(_)
        | TranscriptEntry::Notice(_)
        | TranscriptEntry::Banner(_) => None,
    }
}
