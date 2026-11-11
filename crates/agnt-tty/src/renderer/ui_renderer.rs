use agnt_kernel::SessionRecord;
use agnt_kernel::bus::{LifecycleEvent, NarrationEvent};

/// The drawer's intake: one datum from the concerns it draws from.
///
/// The producer ships records and events; this enum only names which concern
/// arrived. How anything is drawn is this crate's business.
pub enum RenderInput<'a> {
    Record(&'a SessionRecord),
    Narration(&'a NarrationEvent),
    Lifecycle(&'a LifecycleEvent),
}

/// Shared UI renderer contract boundary for agent UI output.
///
/// This trait intentionally lives at `ui/renderer.rs` (UI root) rather than
/// under `ui/tui/` or `ui/stderr/` because it defines the cross-renderer
/// substitution contract used by both concrete renderer domains.
///
/// In other words, this is not a TUI implementation detail — it is the
/// explicit interface boundary between the bus and renderer backends.
pub trait UiRenderer {
    /// Draw one datum the renderer subscribes to.
    fn draw(&mut self, input: RenderInput<'_>);
    fn flush(&mut self);

    /// Advance the renderer's own animation cadence, when one is due.
    ///
    /// The spinner cadence belongs to the renderer, not to the bus: the drain
    /// task that drives this renderer calls it on its interval. A renderer with
    /// no animation leaves the default.
    fn tick(&mut self) {}
}
