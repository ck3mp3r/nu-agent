//! The TUI rendering backend: one `agnt_render::RenderBackend` implementation
//! and the per-family widgets that compose the catalog contract.

// region:    --- Modules

use agnt_render::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

mod backend;
mod banner;
mod components;
mod message;
mod notice;
pub(crate) mod text;
mod tool;

pub use backend::RatatuiBackend;

// endregion: --- Modules

// region:    --- Traits

/// Per-frame paint inputs. The entry selects the widget; the widget reads what
/// it needs from the frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PaintCtx<'a> {
    pub theme: &'a Theme,
    pub now_millis: u128,
}

/// One family widget. `measure` and `paint` must agree row for row: the
/// planner measures, the pane paints.
///
/// Widgets are selected by the entry enum and painted through this bound, so
/// dispatch stays static.
pub(crate) trait PaneWidget {
    /// Visual rows this widget renders at `width`.
    fn measure(&self, width: usize) -> usize;

    /// Paint the widget into `area`, which is exactly `measure` rows tall.
    fn paint(&self, area: Rect, buf: &mut Buffer, ctx: PaintCtx<'_>);
}

// endregion: --- Traits

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/widgets/test.rs"]
mod test;

// endregion: --- Tests
