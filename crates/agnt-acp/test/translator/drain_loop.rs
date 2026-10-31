//! Drain-loop behaviour: streaming, flushing, and transport failure.

use agent_client_protocol::schema::v1::SessionUpdate;
use agnt_core::protocol::events::SystemEvent;

use super::support::*;
use crate::translator::{
    EventTranslator, drain_bus_to_notifications, drain_receiver_to_notifications,
    run_turn_with_notifications,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_translator_run_turn_streams_events_and_returns_turn_output() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
    let publisher = bus.clone();

    // -- Exec
    let (output, _) = run_turn_with_notifications(
        &mut rx,
        &mut None,
        &sink,
        &mut translator,
        &mut bridge,
        async move {
            publisher
                .ui_event()
                .send(SystemEvent::InferenceChunk {
                    delta: "streamed".to_string(),
                    aggregated: "streamed".to_string(),
                })
                .await
                .map_err(|e| format!("should publish: {e:?}"))?;
            publisher
                .ui_event()
                .send(SystemEvent::Tick)
                .await
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
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());

    // -- Exec
    let (output, _) = run_turn_with_notifications(
        &mut rx,
        &mut None,
        &sink,
        &mut translator,
        &mut bridge,
        async {
            bus.ui_event()
                .send(SystemEvent::InferenceChunk {
                    delta: "final".to_string(),
                    aggregated: "final".to_string(),
                })
                .await
                .ok();
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
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingSink::failing();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
    let publisher = bus.clone();

    // -- Exec
    let (output, _) = run_turn_with_notifications(
        &mut rx,
        &mut None,
        &sink,
        &mut translator,
        &mut bridge,
        async move {
            publisher
                .ui_event()
                .send(SystemEvent::InferenceChunk {
                    delta: "dropped".to_string(),
                    aggregated: "dropped".to_string(),
                })
                .await
                .ok();
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

#[tokio::test(start_paused = true)]
async fn test_translator_run_turn_stops_polling_a_closed_compaction_channel() -> Result<()> {
    // -- Setup & Fixtures: a compaction channel whose sender is already gone,
    // so its receiver reports Closed before the drain starts.
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let compaction_tx = agnt_core::bus::CompactionTx::new("compaction", 16);
    let compaction_rx = compaction_tx.subscribe();
    drop(compaction_tx);
    let mut compaction_rx = Some(compaction_rx);
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());

    // -- Exec: the turn completes only once the paused timer advances, which
    // needs the drain to go idle. A drain that keeps polling the closed
    // channel never yields, so the timer never fires and this hangs.
    let (output, _) = run_turn_with_notifications(
        &mut rx,
        &mut compaction_rx,
        &sink,
        &mut translator,
        &mut bridge,
        async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            "done"
        },
    )
    .await;

    // -- Check
    assert_eq!(
        output, "done",
        "the turn must complete even though the compaction channel is closed"
    );
    Ok(())
}

#[tokio::test]
async fn test_translator_run_turn_reports_a_compaction_request() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let mut compaction_rx = Some(bus.compaction().subscribe());
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
    let publisher = bus.clone();

    // -- Exec
    let (output, requested) = run_turn_with_notifications(
        &mut rx,
        &mut compaction_rx,
        &sink,
        &mut translator,
        &mut bridge,
        async move {
            publisher
                .compaction()
                .send(SystemEvent::CompactionRequested {
                    source: "auto".to_string(),
                })
                .await
                .ok();
            "done"
        },
    )
    .await;

    // -- Check: the request is reported to the caller, not to the client.
    assert_eq!(output, "done");
    assert!(
        requested,
        "a Requested event must be reported so the caller can dispatch it"
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
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let mut compaction_rx = Some(bus.compaction().subscribe());
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());

    // -- Exec
    let (output, requested) = run_turn_with_notifications(
        &mut rx,
        &mut compaction_rx,
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
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
    bus.ui_event()
        .send(SystemEvent::InferenceChunk {
            delta: "one".to_string(),
            aggregated: "one".to_string(),
        })
        .await
        .map_err(|e| format!("should publish: {e:?}"))?;
    drop(bus);

    // -- Exec
    drain_receiver_to_notifications(&mut rx, &sink, &mut translator, &mut bridge).await;

    // -- Check
    assert_eq!(sink.taken().len(), 1);
    Ok(())
}

#[tokio::test]
async fn test_translator_drain_bus_subscribes_and_forwards_events() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let sink = std::sync::Arc::new(RecordingSink::default());
    let publisher = bus.clone();
    let drain_sink = std::sync::Arc::clone(&sink);
    let drain = tokio::spawn(async move {
        let mut translator = EventTranslator::new(SESSION);
        let mut bridge = permission_bridge(selected("allow-once"), empty_pending());
        drain_bus_to_notifications(
            &publisher,
            drain_sink.as_ref(),
            &mut translator,
            &mut bridge,
        )
        .await;
    });
    // -- Exec
    // The drain subscribes on its first poll. A broadcast send succeeds only
    // once a receiver is subscribed, so retry until the drain is listening.
    let event = SystemEvent::InferenceChunk {
        delta: "live".to_string(),
        aggregated: "live".to_string(),
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if bus.ui_event().send(event.clone()).await.is_ok() {
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
