//! Test support for the widget-backend suite: the frame-reset mutator.
//!
//! `clear` has no src caller — the widget tests reset the painted surface
//! between frames — so it lives here, not in `src`. This module is declared from
//! `src/widgets/backend.rs`, which makes it a descendant of the module that owns
//! the private `buffer` field.

use super::RatatuiBackend;

impl RatatuiBackend {
    /// Drop every painted cell, so the backend can paint a fresh frame.
    pub(crate) fn clear(&mut self) {
        self.buffer.reset();
    }
}
