//! Completed-turn dispatch: token tracking and UI event emission.

use super::*;

#[tokio::test]
async fn dispatch_completed_turn_emits_the_assistant_message_when_no_deltas() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-completed";
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::create_bus();
    let mut collector = BusEventCollector::subscribe(&bus);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );
    let mut result = completed_result("final answer");
    result.deltas_emitted = false;

    // -- Exec
    let outcome = executor
        .dispatch_result(Ok(result), nu_protocol::Span::test_data(), Some(session_id))
        .await
        .map_err(|e| format!("a completed turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::Completed));
    assert_eq!(
        memory_state.last_total_tokens(),
        Some(12),
        "a completed turn must record the last total token count"
    );
    let events = collector.drain();
    assert!(
        events.iter().any(|e| matches!(
            e,
            UiEvent::AssistantMessage { text, .. } if text == "final answer"
        )),
        "a non-streamed turn must emit the assistant message; got: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn dispatch_completed_turn_skips_the_assistant_message_when_deltas_were_emitted() -> Result<()>
{
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-completed-streamed";
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::create_bus();
    let mut collector = BusEventCollector::subscribe(&bus);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(completed_result("streamed answer")),
            nu_protocol::Span::test_data(),
            Some(session_id),
        )
        .await
        .map_err(|e| format!("a completed turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::Completed));
    let events = collector.drain();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, UiEvent::AssistantMessage { .. })),
        "a streamed turn must not re-emit the assistant message; got: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, UiEvent::Completed { .. })),
        "a completed turn must emit a completion; got: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn dispatch_completed_turn_without_a_session_skips_token_tracking() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(completed_result("sessionless answer")),
            nu_protocol::Span::test_data(),
            None,
        )
        .await
        .map_err(|e| format!("a completed turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::Completed));
    assert_eq!(
        memory_state.last_total_tokens(),
        None,
        "a sessionless turn must not record a token count"
    );
    Ok(())
}
