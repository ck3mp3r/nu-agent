use crate::bus::{CompactionEvent, LlmEvent, ToolEvent, TurnEvent, WarningEvent, create_bus};
use crate::orchestrator::bridge::{BridgeAction, bridge_action};
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

/// A `ProgressUi` that mirrors the worker path: every emitted `UiEvent` is fed
/// through `bridge_action`, and the bridge's published events are re-published
/// onto the same bus `BusForwarder` drains. This reproduces the closed circuit
/// that previously re-injected lifecycle events and repeated the transcript.
///
/// This is a test harness for observable behaviour only — it asserts through the
/// public `ProgressUi`/`BusForwarder`/`bridge_action` boundary, not any backdoor.
struct CircuitSink {
    bus: crate::bus::Bus,
    emitted: Vec<UiEvent>,
}

impl CircuitSink {
    fn new(bus: crate::bus::Bus) -> Self {
        Self {
            bus,
            emitted: Vec::new(),
        }
    }
}

impl ProgressUi for CircuitSink {
    fn emit(&mut self, event: &UiEvent) {
        self.emitted.push(event.clone());
        match bridge_action(event.clone()) {
            BridgeAction::PublishPermission(permission) => {
                let _ = self.bus.permission().send(permission);
            }
            BridgeAction::Ignore => {}
        }
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
    bus.llm().send(LlmEvent::Started).unwrap();
    bus.llm()
        .send(LlmEvent::AssistantMessage {
            text: "hello".to_string(),
        })
        .unwrap();

    let mut ui = SinkUi::new();
    forwarder.drain_to(&mut ui);

    assert!(matches!(ui.events[0], UiEvent::LlmStarted));
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
        .send(ToolEvent::Started {
            name: "read".to_string(),
            source: "builtin".to_string(),
            arguments: "{}".to_string(),
        })
        .unwrap();

    let mut ui = SinkUi::new();
    forwarder.drain_to(&mut ui);

    assert!(matches!(&ui.events[0], UiEvent::ToolStarted { name, .. } if name == "read"));
}

#[test]
fn drains_turn_and_warning_events() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.turn()
        .send(TurnEvent::Completed { tool_calls: 2 })
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
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: "s".to_string(),
            summary_body: "s".to_string(),
        })
        .unwrap();

    let mut ui = SinkUi::new();
    forwarder.drain_to(&mut ui);

    assert!(matches!(
        &ui.events[0],
        UiEvent::CompactionCompleted { source, .. } if source == "auto"
    ));
}

// ---------------------------------------------------------------------------
// Feedback-loop regression tests.
//
// A lifecycle event published to the bus is drained by BusForwarder and emitted
// to the UI. If the UI feeds that emission back through the worker bridge into
// the bus, the loop repeats the event forever. The fix makes the bridge Ignore
// lifecycle events, so each event is emitted exactly once and never re-enters
// the bus.
// ---------------------------------------------------------------------------

#[test]
fn tool_event_is_emitted_exactly_once_and_not_republished() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.tool()
        .send(ToolEvent::Started {
            name: "read".to_string(),
            source: "builtin".to_string(),
            arguments: "{}".to_string(),
        })
        .unwrap();

    let mut ui = CircuitSink::new(bus.clone());
    forwarder.drain_to(&mut ui);
    forwarder.drain_to(&mut ui);

    assert_eq!(
        ui.emitted.len(),
        1,
        "tool start must be emitted exactly once"
    );
    assert!(matches!(&ui.emitted[0], UiEvent::ToolStarted { name, .. } if name == "read"));
}

#[test]
fn llm_event_is_emitted_exactly_once_and_not_republished() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.llm()
        .send(LlmEvent::AssistantMessage {
            text: "hello".to_string(),
        })
        .unwrap();

    let mut ui = CircuitSink::new(bus.clone());
    forwarder.drain_to(&mut ui);
    forwarder.drain_to(&mut ui);

    assert_eq!(
        ui.emitted.len(),
        1,
        "llm message must be emitted exactly once"
    );
    assert!(matches!(
        &ui.emitted[0],
        UiEvent::AssistantMessage { text } if text == "hello"
    ));
}

#[test]
fn turn_completion_is_emitted_exactly_once_and_not_republished() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.turn()
        .send(TurnEvent::Completed { tool_calls: 2 })
        .unwrap();

    let mut ui = CircuitSink::new(bus.clone());
    forwarder.drain_to(&mut ui);
    forwarder.drain_to(&mut ui);

    assert_eq!(
        ui.emitted.len(),
        1,
        "turn completion must be emitted exactly once"
    );
    assert!(matches!(
        ui.emitted[0],
        UiEvent::Completed { tool_calls: 2 }
    ));
}

#[test]
fn warning_is_emitted_exactly_once_and_not_republished() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.warning()
        .send(WarningEvent::Message {
            message: "warn".to_string(),
        })
        .unwrap();

    let mut ui = CircuitSink::new(bus.clone());
    forwarder.drain_to(&mut ui);
    forwarder.drain_to(&mut ui);

    assert_eq!(ui.emitted.len(), 1, "warning must be emitted exactly once");
    assert!(matches!(&ui.emitted[0], UiEvent::Warning { message } if message == "warn"));
}

#[test]
fn compaction_event_is_emitted_exactly_once_and_not_republished() {
    let bus = create_bus();
    let mut forwarder = BusForwarder::new(&bus);
    bus.compaction()
        .send(CompactionEvent::Completed {
            source: "auto".to_string(),
            summary_preview: "s".to_string(),
            summary_body: "s".to_string(),
        })
        .unwrap();

    let mut ui = CircuitSink::new(bus.clone());
    forwarder.drain_to(&mut ui);
    forwarder.drain_to(&mut ui);

    assert_eq!(
        ui.emitted.len(),
        1,
        "compaction must be emitted exactly once"
    );
    assert!(matches!(
        &ui.emitted[0],
        UiEvent::CompactionCompleted { source, .. } if source == "auto"
    ));
}
