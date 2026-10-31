use std::sync::{Arc, Mutex};

use super::*;

use agnt_core::protocol::events::SystemEvent;
use agnt_core::renderer::UiRenderer;

#[derive(Default)]
pub(crate) struct FakeRenderer {
    emitted: Vec<SystemEvent>,
    flushed: usize,
}

impl UiRenderer for FakeRenderer {
    fn emit(&mut self, event: &SystemEvent) {
        self.emitted.push(event.clone());
    }

    fn flush(&mut self) {
        self.flushed += 1;
    }
}

pub(crate) struct CapturingRenderer {
    events: Arc<Mutex<Vec<SystemEvent>>>,
}

impl CapturingRenderer {
    pub(crate) fn new(events: Arc<Mutex<Vec<SystemEvent>>>) -> Self {
        Self { events }
    }
}

impl UiRenderer for CapturingRenderer {
    fn emit(&mut self, event: &SystemEvent) {
        self.events.lock().expect("events").push(event.clone());
    }

    fn flush(&mut self) {}
}

impl<R: UiRenderer, E: TerminalEventSource> TuiRuntimeRenderer<R, E> {
    pub(crate) fn new_tui_active_for_test(
        inner: R,
        event_source: E,
        columns: u16,
        rows: u16,
    ) -> Self {
        Self::with_terminal_mode(inner, event_source, columns, rows, None, true)
    }

    pub(crate) fn coordinator(&self) -> &RuntimeCoordinator {
        &self.coordinator
    }
}
