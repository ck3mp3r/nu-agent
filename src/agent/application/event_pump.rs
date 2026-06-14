use std::sync::mpsc;

use crate::agent::protocol::contracts::InteractiveUi;
#[cfg(test)]
use crate::agent::protocol::contracts::ProgressUi;
use crate::agent::protocol::event::UiEvent;

pub(crate) struct EventPump {
    event_rx: mpsc::Receiver<UiEvent>,
}

impl EventPump {
    pub(crate) fn new(event_rx: mpsc::Receiver<UiEvent>) -> Self {
        Self { event_rx }
    }

    /// Forward all pending events from the worker channel to the UI.
    /// Returns the number of events forwarded.
    #[cfg(test)]
    pub(crate) fn drain<U: ProgressUi>(&mut self, ui: &mut U) -> usize {
        let mut count = 0;
        while let Ok(event) = self.event_rx.try_recv() {
            ui.emit(&event);
            count += 1;
        }
        count
    }

    /// Collect all pending events and forward them as a batch via
    /// [`InteractiveUi::emit_batch`]. Returns the number of events forwarded.
    pub(crate) fn drain_batch<U: InteractiveUi>(&mut self, ui: &mut U) -> usize {
        let mut batch = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            batch.push(event);
        }
        let count = batch.len();
        if !batch.is_empty() {
            log::debug!("orchestrator: forwarding {} worker events to UI", count);
            ui.emit_batch(&batch);
        }
        count
    }
}

#[cfg(test)]
#[path = "event_pump_test.rs"]
mod event_pump_test;
