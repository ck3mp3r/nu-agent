//! The loop's ending hands: the quit and fatal state the surface publishes, the
//! delivery deadline that bounds a blocked stop, and the cancel intake.

use super::RuntimeCoordinator;

impl RuntimeCoordinator {
    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn fatal_error(&self) -> Option<&str> {
        self.fatal_error.as_deref()
    }

    /// Track a quit or fatal delivery the orchestrator has not taken yet.
    ///
    /// The deadline is armed on the first blocked attempt and measured on the
    /// tokio clock, so a paused-time test advances it deterministically. Past
    /// the deadline the consumer is wedged, and exiting is the only recovery.
    pub(crate) fn quit_delivery_expired(&mut self) -> bool {
        let deadline = *self
            .quit_delivery_deadline
            .get_or_insert_with(|| tokio::time::Instant::now() + Self::QUIT_DELIVERY_GIVE_UP);
        tokio::time::Instant::now() >= deadline
    }

    /// Forget a delivery deadline: the event landed, so the retry is over.
    pub(crate) fn clear_quit_delivery_deadline(&mut self) {
        self.quit_delivery_deadline = None;
    }

    pub fn event_sender(
        &self,
    ) -> &tokio::sync::mpsc::Sender<agnt_runtime::orchestrator::events::OrchestratorEvent> {
        &self.state.event_tx
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.cancel_controller.take_cancel_requested()
    }
}
