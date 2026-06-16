use std::sync::mpsc;

use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;

pub struct EventPump {
    event_rx: mpsc::Receiver<UiEvent>,
}

impl EventPump {
    pub fn new(event_rx: mpsc::Receiver<UiEvent>) -> Self {
        Self { event_rx }
    }

    /// Forward all pending events from the worker channel to the UI.
    /// Returns the number of events forwarded.
    #[cfg(test)]
    pub fn drain<U: ProgressUi>(&mut self, ui: &mut U) -> usize {
        let mut count = 0;
        while let Ok(event) = self.event_rx.try_recv() {
            ui.emit(&event);
            count += 1;
        }
        count
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
            log::debug!("orchestrator: forwarding {} worker events to UI", count);
            ui.emit_batch(&batch);
        }
        count
    }
}

#[cfg(test)]
#[path = "pump_test.rs"]
mod pump_test;
