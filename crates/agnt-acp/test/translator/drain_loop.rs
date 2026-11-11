//! Drain-loop behaviour: streaming, flushing, and transport failure.

use agent_client_protocol::schema::v1::SessionUpdate;
use agnt_runtime::bus::{ControlCommand, NarrationEvent};
use agnt_runtime::protocol::events::CompactionSource;

use super::support::*;
use crate::translator::{
    EventTranslator, TurnSources, drain_bus_to_notifications, drain_receiver_to_notifications,
    run_turn_with_notifications,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// One streamed inference delta, as the narration concern carries it.
fn chunk(text: &str) -> NarrationEvent {
    NarrationEvent::InferenceChunk {
        delta: text.to_string(),
        aggregated: text.to_string(),
    }
}

#[tokio::test]
async fn test_translator_run_turn_streams_events_and_returns_turn_output() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let mut stream_rx = bus.stream().subscribe();
    let mut narration_rx = bus.narration().subscribe();
    let mut control_rx = bus.control().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
    let publisher = bus.clone();

    // -- Exec
    let (output, _) = run_turn_with_notifications(
        &mut stream_rx,
        &mut narration_rx,
        &mut control_rx,
        &sink,
        &mut translator,
        &mut bridge,
        async move {
            publisher
                .narration()
                .publish(chunk("streamed"))
                .map_err(|e| format!("should publish: {e:?}"))?;
            Ok::<&str, String>("turn-done")
        },
    )
    .await;

    // -- Check
    assert_eq!(output, Ok("turn-done"));
    let updates = sink.taken();
    assert_eq!(updates.len(), 1, "only translatable events must be sent");
    assert!(matches!(updates[0], SessionUpdate::AgentMessageChunk(_)));
    Ok(())
}

#[tokio::test]
async fn test_translator_run_turn_flushes_events_published_before_completion() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let mut stream_rx = bus.stream().subscribe();
    let mut narration_rx = bus.narration().subscribe();
    let mut control_rx = bus.control().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());

    // -- Exec
    let (output, _) = run_turn_with_notifications(
        &mut stream_rx,
        &mut narration_rx,
        &mut control_rx,
        &sink,
        &mut translator,
        &mut bridge,
        async {
            bus.narration().publish(chunk("final")).ok();
            "done"
        },
    )
    .await;

    // -- Check
    assert_eq!(output, "done");
    assert_eq!(
        sink.taken().len(),
        1,
        "the final event must be flushed after the turn completes"
    );
    Ok(())
}

#[tokio::test]
async fn test_translator_run_turn_stops_draining_when_sink_fails() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let mut stream_rx = bus.stream().subscribe();
    let mut narration_rx = bus.narration().subscribe();
    let mut control_rx = bus.control().subscribe();
    let sink = RecordingSink::failing();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
    let publisher = bus.clone();

    // -- Exec
    let (output, _) = run_turn_with_notifications(
        &mut stream_rx,
        &mut narration_rx,
        &mut control_rx,
        &sink,
        &mut translator,
        &mut bridge,
        async move {
            publisher.narration().publish(chunk("dropped")).ok();
            "done"
        },
    )
    .await;

    // -- Check
    assert_eq!(output, "done");
    assert!(
        sink.taken().is_empty(),
        "a failing sink records nothing and must not block the turn"
    );
    Ok(())
}

#[tokio::test]
async fn test_translator_run_turn_reports_a_compaction_request() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let mut stream_rx = bus.stream().subscribe();
    let mut narration_rx = bus.narration().subscribe();
    let mut control_rx = bus.control().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
    let publisher = bus.clone();

    // -- Exec
    let (output, requested) = run_turn_with_notifications(
        &mut stream_rx,
        &mut narration_rx,
        &mut control_rx,
        &sink,
        &mut translator,
        &mut bridge,
        async move {
            publisher
                .control()
                .publish(ControlCommand::CompactionRequested {
                    source: CompactionSource::Auto,
                })
                .ok();
            "done"
        },
    )
    .await;

    // -- Check: the request is reported to the caller, not to the client.
    assert_eq!(output, "done");
    assert!(
        requested,
        "a Requested command must be reported so the caller can dispatch it"
    );
    assert!(
        sink.taken().is_empty(),
        "the request is not a client-facing event"
    );
    Ok(())
}

#[tokio::test]
async fn test_translator_run_turn_reports_no_compaction_request() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let mut stream_rx = bus.stream().subscribe();
    let mut narration_rx = bus.narration().subscribe();
    let mut control_rx = bus.control().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());

    // -- Exec
    let (output, requested) = run_turn_with_notifications(
        &mut stream_rx,
        &mut narration_rx,
        &mut control_rx,
        &sink,
        &mut translator,
        &mut bridge,
        async { "done" },
    )
    .await;

    // -- Check
    assert_eq!(output, "done");
    assert!(
        !requested,
        "a turn that requests no compaction must report none"
    );
    Ok(())
}

#[tokio::test]
async fn test_translator_drain_receiver_stops_on_closed_bus() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let mut stream_rx = bus.stream().subscribe();
    let mut narration_rx = bus.narration().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
    bus.narration()
        .publish(chunk("one"))
        .map_err(|e| format!("should publish: {e:?}"))?;
    drop(bus);

    // -- Exec
    drain_receiver_to_notifications(
        &mut stream_rx,
        &mut narration_rx,
        &sink,
        &mut translator,
        &mut bridge,
    )
    .await;

    // -- Check
    assert_eq!(sink.taken().len(), 1);
    Ok(())
}

#[tokio::test]
async fn test_translator_drain_bus_subscribes_and_forwards_events() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let sink = std::sync::Arc::new(RecordingSink::default());
    let publisher = bus.clone();
    let drain_sink = std::sync::Arc::clone(&sink);
    let drain = tokio::spawn(async move {
        let mut translator = EventTranslator::new(SESSION);
        let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
        drain_bus_to_notifications(
            TurnSources {
                stream: publisher.stream(),
                narration: publisher.narration(),
                control: publisher.control(),
            },
            drain_sink.as_ref(),
            &mut translator,
            &mut bridge,
        )
        .await;
    });
    // -- Exec
    // The drain subscribes on its first poll. A ring publish succeeds only
    // once a receiver is subscribed, so retry until the drain is listening.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if bus.narration().publish(chunk("live")).is_ok() {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("the drain never subscribed to the bus".into());
        }
        tokio::task::yield_now().await;
    }
    // Await the forwarded notification instead of settling on a fixed delay.
    tokio::time::timeout(std::time::Duration::from_secs(5), sink.wait_for_updates(1))
        .await
        .map_err(|_| "the drain never forwarded the event")?;
    drain.abort();

    // -- Check
    let updates = sink.taken();
    assert_eq!(updates.len(), 1);
    assert!(matches!(updates[0], SessionUpdate::AgentMessageChunk(_)));
    Ok(())
}
