use crate::bus::{Bus, create_bus};
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
fn drain_forwards_all_pending_permission_events_to_ui() {
    let bus: Bus = create_bus();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx, &bus);
    tx.send(UiEvent::LlmStart).ok();
    tx.send(UiEvent::Completed { tool_calls: 0 }).ok();
    let count = pump.drain_batch(&mut ui);
    assert_eq!(count, 2);
    assert_eq!(ui.events.len(), 2);
}

#[test]
fn drain_forwards_bus_tool_events_to_ui() {
    let bus: Bus = create_bus();
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx, &bus);
    bus.tool()
        .send(crate::bus::ToolEvent::Start {
            name: "read".into(),
            source: "user".into(),
            arguments: "{}".into(),
        })
        .expect("send should succeed");
    bus.tool()
        .send(crate::bus::ToolEvent::End {
            name: "read".into(),
            source: "user".into(),
            arguments: "{}".into(),
            success: true,
            result: "ok".into(),
            display: None,
            error_kind: None,
            message: None,
        })
        .expect("send should succeed");
    let count = pump.drain_batch(&mut ui);
    assert_eq!(count, 2);
    assert!(matches!(ui.events[0], UiEvent::ToolStart { .. }));
    assert!(matches!(ui.events[1], UiEvent::ToolEnd { .. }));
}

#[test]
fn drain_forwards_bus_llm_events_to_ui() {
    let bus: Bus = create_bus();
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx, &bus);
    bus.llm()
        .send(crate::bus::LlmEvent::Start)
        .expect("send should succeed");
    bus.llm()
        .send(crate::bus::LlmEvent::End {
            response_chars: 10,
            tool_calls: 0,
            input_tokens: 1,
            output_tokens: 2,
            total_tokens: 3,
        })
        .expect("send should succeed");
    let count = pump.drain_batch(&mut ui);
    assert_eq!(count, 2);
    assert!(matches!(ui.events[0], UiEvent::LlmStart));
    assert!(matches!(ui.events[1], UiEvent::LlmEnd { .. }));
}

#[test]
fn drain_forwards_bus_warning_events_to_ui() {
    let bus: Bus = create_bus();
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx, &bus);
    bus.warning()
        .send(crate::bus::WarningEvent::Message {
            message: "hello".into(),
        })
        .expect("send should succeed");
    let count = pump.drain_batch(&mut ui);
    assert_eq!(count, 1);
    assert!(matches!(ui.events[0], UiEvent::Warning { .. }));
}

#[test]
fn drain_forwards_bus_turn_completed_to_ui() {
    let bus: Bus = create_bus();
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx, &bus);
    bus.turn()
        .send(crate::bus::TurnEvent::TurnCompleted { tool_calls: 1 })
        .expect("send should succeed");
    let count = pump.drain_batch(&mut ui);
    assert_eq!(count, 1);
    assert!(matches!(ui.events[0], UiEvent::Completed { .. }));
}

#[test]
fn drain_returns_zero_when_all_sources_empty() {
    let bus: Bus = create_bus();
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx, &bus);
    assert_eq!(pump.drain_batch(&mut ui), 0);
    assert!(ui.events.is_empty());
}

#[test]
fn drain_on_disconnected_mpsc_returns_zero_when_bus_empty() {
    let bus: Bus = create_bus();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();
    drop(tx);
    let mut ui = CollectingProgressUi::new();
    let mut pump = EventPump::new(rx, &bus);
    assert_eq!(pump.drain_batch(&mut ui), 0);
}
