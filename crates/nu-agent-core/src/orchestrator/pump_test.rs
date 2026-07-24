use crate::orchestrator::pump::EventPump;
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;

struct CollectingProgressUi {
    events: Vec<UiEvent>,
}
impl CollectingProgressUi {
    fn new() -> Self {
        Self { events: vec![] }
    }
}
impl ProgressUi for CollectingProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }
    fn flush(&mut self) {}
    fn take_cancel_requested(&self) -> bool {
        false
    }
}

#[test]
fn drain_forwards_all_pending_events_to_ui() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx);
    tx.send(UiEvent::LlmStart).ok();
    tx.send(UiEvent::Completed { tool_calls: 0 }).ok();
    let count = pump.drain(&mut ui);
    assert_eq!(count, 2);
    assert_eq!(ui.events.len(), 2);
}

#[test]
fn drain_returns_zero_when_channel_empty() {
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx);
    assert_eq!(pump.drain(&mut ui), 0);
    assert!(ui.events.is_empty());
}

#[test]
fn drain_on_disconnected_channel_returns_zero() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    drop(tx);
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx);
    assert_eq!(pump.drain(&mut ui), 0);
}
