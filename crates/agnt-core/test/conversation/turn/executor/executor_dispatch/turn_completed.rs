//! `SystemEvent::TurnCompleted` emission: both turn-end paths publish the turn's
//! tool-call count, its accumulated LLM usage, and its wall-clock span.
//!
//! The turn-completion fact rides the ui-event channel the TUI and the hook
//! runner subscribe to; the turn channel carries only the lifecycle start.

use super::*;

/// Drain every event currently buffered on the ui-event channel.
fn drain_turn_events(rx: &mut crate::bus::UiEventRx) -> Vec<SystemEvent> {
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

/// The fields the turn-completed event carries, in declaration order.
type CompletedFields = (usize, u64, u64, Option<u64>, Option<f64>, bool, u64);

/// The first `SystemEvent::TurnCompleted` in `events`, as its fields.
fn completed_fields(events: &[SystemEvent]) -> Option<CompletedFields> {
    events.iter().find_map(|event| match event {
        SystemEvent::TurnCompleted {
            tool_calls,
            input_tokens,
            output_tokens,
            total_tokens,
            cost,
            cost_complete,
            duration_ms,
        } => Some((
            *tool_calls,
            *input_tokens,
            *output_tokens,
            *total_tokens,
            *cost,
            *cost_complete,
            *duration_ms,
        )),
        _ => None,
    })
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
    let mut turn_rx = bus.ui_event().subscribe();
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
            std::time::Instant::now(),
        )
        .await
        .map_err(|e| format!("a completed turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::Completed));
    let events = drain_turn_events(&mut turn_rx);
    let (tool_calls, input_tokens, output_tokens, total_tokens, cost, _, _) =
        completed_fields(&events).ok_or("a completed turn must publish TurnEvent::Completed")?;
    assert_eq!(
        tool_calls, 3,
        "a completed turn must publish the tool-call count; got: {events:?}"
    );
    assert_eq!(
        (input_tokens, output_tokens, total_tokens, cost),
        (0, 0, None, None),
        "a turn with no LLM calls reports zero sums and no cost"
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
    let mut turn_rx = bus.ui_event().subscribe();
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
            std::time::Instant::now(),
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));
    let events = drain_turn_events(&mut turn_rx);
    assert!(
        events.iter().any(
            |e| matches!(e, SystemEvent::TurnCompleted { tool_calls, .. } if *tool_calls == 2)
        ),
        "the early-return path must publish TurnEvent::Completed with the tool-call count; got: {events:?}"
    );
    Ok(())
}

/// Both turn-end paths publish the turn's accumulated usage: the sums the hook
/// chain wrote as each call completed, plus the turn's wall-clock span.
#[tokio::test]
async fn dispatch_completed_turn_publishes_the_accumulated_usage_and_duration() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-turn-usage";
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::create_bus();
    let mut turn_rx = bus.ui_event().subscribe();
    let infra = default_tool_infra(bus);
    // Two calls. The first's wire omitted its total, so the chain passed input
    // + output; the second reported one and an incomplete cost.
    infra.turn_usage.add_call(10, 5, Some(15), Some(0.1), true);
    infra.turn_usage.add_call(40, 2, Some(42), Some(0.2), false);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        infra,
    );
    let mut result = completed_result("final answer");
    result.tool_call_count = 2;

    // -- Exec
    executor
        .dispatch_result(
            Ok(result),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            Some(session_id),
            std::time::Instant::now(),
        )
        .await
        .map_err(|e| format!("a completed turn must not be an error: {e}"))?;

    // -- Check
    let events = drain_turn_events(&mut turn_rx);
    let (tool_calls, input_tokens, output_tokens, total_tokens, cost, cost_complete, _) =
        completed_fields(&events).ok_or("a completed turn must publish TurnEvent::Completed")?;
    assert_eq!(tool_calls, 2);
    assert_eq!(input_tokens, 50, "input tokens sum across the calls");
    assert_eq!(output_tokens, 7, "output tokens sum across the calls");
    assert_eq!(total_tokens, Some(57), "totals sum across the calls");
    assert!(
        cost.is_some_and(|cost| (cost - 0.3).abs() < 1e-12),
        "reported costs sum; got {cost:?}"
    );
    assert!(
        !cost_complete,
        "one incomplete component makes the sum a lower bound"
    );
    Ok(())
}
