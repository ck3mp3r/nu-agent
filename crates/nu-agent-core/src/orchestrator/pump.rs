use tokio::sync::mpsc;

use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;

pub struct EventPump {
    event_rx: mpsc::UnboundedReceiver<UiEvent>,
}

impl EventPump {
    pub fn new(event_rx: mpsc::UnboundedReceiver<UiEvent>) -> Self {
        Self { event_rx }
    }

    /// Collect all pending events and forward them as a batch via
    /// [`ProgressUi::emit_batch`]. Returns the number of events forwarded.
    pub fn drain_batch<U: ProgressUi>(&mut self, ui: &mut U) -> usize {
        let mut batch = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            batch.push(event);
        }
        let count = batch.len();
        if !batch.is_empty() {
            log::trace!("orchestrator: forwarding {} worker events to UI", count);
            ui.emit_batch(&batch);
        }
        count
    }
}
