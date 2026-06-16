use crate::protocol::event::UiEvent;

/// Shared UI renderer contract boundary for agent UI output.
///
/// This trait intentionally lives at `ui/renderer.rs` (UI root) rather than
/// under `ui/tui/` or `ui/stderr/` because it defines the cross-renderer
/// substitution contract used by both concrete renderer domains.
///
/// In other words, this is not a TUI implementation detail — it is the
/// explicit interface boundary between UI event production and renderer
/// backends.
pub trait UiRenderer {
    fn emit(&mut self, event: &UiEvent);
    fn flush(&mut self);
}
