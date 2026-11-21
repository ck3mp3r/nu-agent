//! The cells whose glyph reads the frame clock, and the painted frame they
//! belong to.
//!
//! A spinner step changes nothing in the state: every byte the last frame left
//! on the surface still holds, except the glyphs that name the current time. The
//! loop therefore records those cells when it composes a frame, and a clock-only
//! frame restores the frame and repaints them. The bytes are the same as a full
//! recomposition would produce; the work is one buffer copy and one cell per
//! clock glyph.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;

use agnt_render::transcript::renderer::ItemStatus;

use crate::runtime::RuntimeCoordinator;
use crate::runtime::status::format::status_indicator;

// region:    --- Types

/// A glyph source that reads the frame clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClockGlyph {
    /// The status bar's busy spinner.
    Session,
    /// One transcript entry's status indicator.
    Item(ItemStatus),
}

impl ClockGlyph {
    /// The glyph this source shows at `now_millis`.
    pub(crate) fn char_at(self, now_millis: u128) -> &'static str {
        match self {
            ClockGlyph::Session => status_indicator(Some(now_millis)),
            ClockGlyph::Item(status) => status.indicator_char(now_millis),
        }
    }
}

/// One cell whose glyph reads the frame clock, with the style the frame painted
/// it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockCell {
    /// Where the cell sits in the frame.
    pub(crate) position: Position,
    /// The glyph the cell shows at a clock reading.
    pub(crate) glyph: ClockGlyph,
    /// The style the cell carries. It is theme-derived, and the theme cannot
    /// change under a clock-only frame: a theme switch recomposes the frame.
    pub(crate) style: Style,
}

impl ClockCell {
    /// The same cell moved by `area`'s origin.
    pub(crate) fn offset_by(self, area: Rect) -> Self {
        Self {
            position: Position {
                x: area.x.saturating_add(self.position.x),
                y: area.y.saturating_add(self.position.y),
            },
            ..self
        }
    }
}

/// One painted frame: the bytes it left on the surface, and the cells that a
/// clock step repaints.
#[derive(Debug)]
pub(crate) struct PaintedFrame {
    buffer: Buffer,
    cells: Vec<ClockCell>,
}

impl PaintedFrame {
    pub(crate) fn new(buffer: Buffer, cells: Vec<ClockCell>) -> Self {
        Self { buffer, cells }
    }

    /// The area the frame covers.
    pub(crate) fn area(&self) -> Rect {
        self.buffer.area
    }

    /// Replace the painted bytes. The clock cells stay: a clock-only frame
    /// changes their glyphs and nothing else, so the next clock step reads the
    /// same cells.
    pub(crate) fn replace_buffer(&mut self, buffer: Buffer) {
        self.buffer = buffer;
    }
}

// endregion: --- Types

// region:    --- RuntimeCoordinator

impl RuntimeCoordinator {
    /// Paint the frame the stored clock reading asks for over the last painted
    /// frame: every cell keeps that frame's bytes, and the cells whose glyph
    /// reads the frame clock take the glyph the clock now asks for.
    ///
    /// Returns whether the patch happened. It does not when no frame has
    /// painted yet or the surface is no longer the size that frame covered, so
    /// the caller composes the whole frame instead.
    pub(crate) fn repaint_clock_cells(&mut self, frame: &mut Frame) -> bool {
        let now_millis = self.frame_clock_millis;
        let Some(painted) = self.last_painted.as_ref() else {
            return false;
        };
        if painted.area() != frame.area() {
            return false;
        }
        frame
            .buffer_mut()
            .content
            .clone_from(&painted.buffer.content);
        for cell in &painted.cells {
            let Some(slot) = frame.buffer_mut().cell_mut(cell.position) else {
                continue;
            };
            slot.set_symbol(cell.glyph.char_at(now_millis));
            slot.set_style(cell.style);
        }
        true
    }
}

// endregion: --- RuntimeCoordinator
