//! Test support for the `TtyBackend` suite: the capture accessors.
//!
//! The backend's painted rows are test-only surface, so the accessors live here,
//! not in `src`. This module is declared from `src/renderer/backend.rs`, which
//! makes it a descendant of the module that owns the private `lines` field.

use crate::renderer::backend::TtyBackend;

impl TtyBackend {
    /// Rows written so far.
    pub(crate) fn output(&self) -> &[String] {
        &self.lines
    }

    /// The output as one newline-joined string.
    pub(crate) fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Drop every row, so the backend can paint a new frame.
    pub(crate) fn clear(&mut self) {
        self.lines.clear();
    }
}
