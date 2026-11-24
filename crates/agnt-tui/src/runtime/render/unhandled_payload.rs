//!
//! The drawer's account of a picker payload it does not enumerate. Each drawing
//! surface names the variant it meets once and paints a defined fallback: a
//! payload the drawer does not know is a rendering decision, never an abort.

use std::cell::Cell;

use crate::state::PickerPayload;

/// The warn-once accounting one drawing surface keeps for one drawn pass.
pub(crate) struct UnhandledPayload {
    surface: &'static str,
    warned: Cell<bool>,
}

impl UnhandledPayload {
    pub(crate) fn new(surface: &'static str) -> Self {
        Self {
            surface,
            warned: Cell::new(false),
        }
    }

    /// Names the payload variant on the first miss of the drawn pass. The
    /// fallback is the caller's: the drawer still derives, it derives a defined
    /// state.
    pub(crate) fn warn(&self, payload: &PickerPayload) {
        if !self.warned.replace(true) {
            log::warn!(
                "tui-unhandled-payload: the {} drawer does not enumerate the {} payload; drawing the fallback",
                self.surface,
                payload.kind_name()
            );
        }
    }
}
