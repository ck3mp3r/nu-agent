//! `TurnEvent::Completed` emission: both turn-end paths publish the turn's
//! tool-call count on the turn channel.
//!
//! The turn channel is the control plane the hook runner subscribes to. The
//! TUI keeps reconstructing `TurnEvent::Completed` from `UiEvent::Completed`,
//! so both events ride their own channels.

use super::*;
use crate::bus::TurnEvent;

/// Drain every event currently buffered on the turn channel.
fn drain_turn_events(rx: &mut crate::bus::TurnRx) -> Vec<TurnEvent> {
    let mut events = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(event) => events.push(event),
            Err(crate::bus::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
    events
}

/// A turn that completes normally publishes `TurnEvent::Completed` carrying
/// `turn_result.tool_call_count`.
#[tokio::test]
async fn dispatch_completed_turn_publishes_turn_completed_with_the_tool_call_count() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-turn-completed";
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::create_bus();
    let mut turn_rx = bus.turn().subscribe();
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );
    let mut result = completed_result("final answer");
    result.tool_call_count = 3;

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(result),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            Some(session_id),
        )
        .await
        .map_err(|e| format!("a completed turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::Completed));
    let events = drain_turn_events(&mut turn_rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, TurnEvent::Completed { tool_calls } if *tool_calls == 3)),
        "a completed turn must publish TurnEvent::Completed with the tool-call count; got: {events:?}"
    );
    Ok(())
}

/// A turn that ends through the early-return cancel/stop path publishes
/// `TurnEvent::Completed` carrying `turn_result.tool_call_count`.
#[tokio::test]
async fn dispatch_cancelled_turn_publishes_turn_completed_with_the_tool_call_count() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::create_bus();
    let mut turn_rx = bus.turn().subscribe();
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );
    let mut result = cancelled_result(None, Vec::new(), None, "");
    result.tool_call_count = 2;

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(result),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            None,
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));
    let events = drain_turn_events(&mut turn_rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, TurnEvent::Completed { tool_calls } if *tool_calls == 2)),
        "the early-return path must publish TurnEvent::Completed with the tool-call count; got: {events:?}"
    );
    Ok(())
}
