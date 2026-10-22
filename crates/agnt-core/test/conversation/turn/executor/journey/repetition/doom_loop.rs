use super::*;

// ---------------------------------------------------------------------------
// Doom-loop journey: 8 identical tool calls → stop on detection 4
// ---------------------------------------------------------------------------

/// A doom-loop journey drives 8 identical tool calls (5 threshold + 1 first +
/// 2 backoff + 1 stop). The turn ends with an EarlyReturn whose response text
/// starts with DOOM_LOOP_STOP_PREFIX and names the looping tool; the bus
/// carries a Warning and a Stopped notice with the same prefix; the stopped
/// run saves nothing.
#[tokio::test]
async fn journey_doom_loop_stop_surfaces_reason() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-doom-loop-stop");

    // 8 identical tool calls: 5 threshold + 1 first + 2 backoff + 1 stop.
    let turns: Vec<Vec<MockStreamEvent>> = (0..8)
        .map(|i| {
            vec![
                MockStreamEvent::tool_call(format!("tc{i}"), "test_echo", serde_json::json!({})),
                MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    let model = MockCompletionModel::from_stream_turns(turns);

    // -- Exec
    let (r, events) = h
        .turn(
            "do the same thing repeatedly",
            model,
            echo_tool("result_42")?,
        )
        .await;

    // -- Check
    let outcome = r.map_err(|e| format!("doom stop must be Ok: {e:?}"))?;
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("doom stop must return EarlyReturn".into());
    };
    let response_text = extract_response_text_from_value(&value);
    assert!(
        response_text.starts_with(DOOM_LOOP_STOP_PREFIX),
        "response text must start with DOOM_LOOP_STOP_PREFIX, got: {response_text}"
    );
    assert!(
        response_text.contains("test_echo"),
        "response text must name the looping tool, got: {response_text}"
    );

    assert!(
        events.iter().any(|e| matches!(e, UiEvent::Warning { message } if message.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "must emit a Warning starting with DOOM_LOOP_STOP_PREFIX; got: {events:?}"
    );
    // Fix 2: the stop reason rides `UiEvent::Stopped` (appended as a notice),
    // never `AssistantMessage` (which would truncate the streamed block).
    assert!(
        events.iter().any(|e| matches!(e, UiEvent::Stopped { reason } if reason.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "must emit a Stopped event starting with DOOM_LOOP_STOP_PREFIX; got: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, UiEvent::AssistantMessage { text, .. } if text.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "the stop reason must NOT ride AssistantMessage; got: {events:?}"
    );

    // The stop is an EarlyReturn, so rig saves nothing: the skip-result
    // ToolResults live only in the run transcript, which the executor does not
    // return.
    let msgs = h.raw_messages().await?;
    assert!(
        msgs.is_empty(),
        "a stopped turn must save nothing; got: {msgs:?}"
    );

    Ok(())
}
