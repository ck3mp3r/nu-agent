use crate::bus::{CompactionEvent, LlmEvent, ToolEvent, TurnEvent, WarningEvent, create_bus};
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;

use super::BusForwarder;

struct SinkUi {
    events: Vec<UiEvent>,
}

impl SinkUi {
    fn new() -> Self {
        Self { events: Vec::new() }
    }
}

impl ProgressUi for SinkUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

#[test]
fn new_subscribes_to_five_channels() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    // Construction succeeds and drains without panicking.
    let mut ui = SinkUi::new();
    forwarder.drain_to(&mut ui);
    assert!(ui.events.is_empty());
}

#[test]
fn drains_llm_events() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.llm().send(LlmEvent::Start).unwrap();
    bus.llm()
        .send(LlmEvent::AssistantMessage {
            text: "hello".to_string(),
        })
        .unwrap();

    let mut ui = SinkUi::new();
    forwarder.drain_to(&mut ui);

    assert!(matches!(ui.events[0], UiEvent::LlmStart));
    assert!(matches!(
        &ui.events[1],
        UiEvent::AssistantMessage { text } if text == "hello"
    ));
}

#[test]
fn drains_tool_events() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.tool()
        .send(ToolEvent::Start {
            name: "read".to_string(),
            source: "builtin".to_string(),
            arguments: "{}".to_string(),
        })
        .unwrap();

    let mut ui = SinkUi::new();
    forwarder.drain_to(&mut ui);

    assert!(matches!(&ui.events[0], UiEvent::ToolStart { name, .. } if name == "read"));
}

#[test]
fn drains_turn_and_warning_events() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.turn()
        .send(TurnEvent::TurnCompleted { tool_calls: 2 })
        .unwrap();
    bus.warning()
        .send(WarningEvent::Message {
            message: "warn".to_string(),
        })
        .unwrap();

    let mut ui = SinkUi::new();
    forwarder.drain_to(&mut ui);

    assert!(matches!(ui.events[0], UiEvent::Completed { tool_calls: 2 }));
    assert!(matches!(&ui.events[1], UiEvent::Warning { message } if message == "warn"));
}

#[test]
fn drains_compaction_events() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.compaction()
        .send(CompactionEvent::Triggered {
            source: "auto".to_string(),
            summarized_count: 3,
            kept_recent_count: 0,
            summary_preview: "s".to_string(),
            summary_body: "s".to_string(),
        })
        .unwrap();

    let mut ui = SinkUi::new();
    forwarder.drain_to(&mut ui);

    assert!(matches!(
        &ui.events[0],
        UiEvent::CompactionTriggered { source, .. } if source == "auto"
    ));
}
