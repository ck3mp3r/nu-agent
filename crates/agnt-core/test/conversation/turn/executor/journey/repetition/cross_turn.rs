use super::*;

// ---------------------------------------------------------------------------
// Output-repetition journeys: cross-turn and intra-stream
// ---------------------------------------------------------------------------

/// Cross-turn: 4 identical text-only completions across separate `execute_turn`
/// calls sharing one `ToolInfra` do not trip. The 5th turn's completion is a
/// First detection — `on_model_turn_finished` returns `retry_with_feedback`,
/// and the retry (same content) escalates Backoff, Backoff, then Stop; the
/// stop-to-steering conversion retries (up to the cap of 3), and the stop at
/// the cap is terminal: the turn ends with an EarlyReturn whose response text
/// is the exact stop text; the executor surfaces it.
#[tokio::test]
async fn journey_output_repetition_cross_turn_trips_on_fifth() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-cross-turn");
    let tool_infra = no_tools();

    // -- Exec: 4 identical text-only turns (no detection), then a 5th turn
    // whose model repeats the same content so the ladder escalates to Stop.
    for _ in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::final_response_with_total_tokens(1),
        ]]);
        let (r, _) = h.turn("do the thing", model, tool_infra.clone()).await;
        let outcome = r.map_err(|e| format!("turn {e:?}"))?;
        assert!(
            matches!(outcome, TurnOutcome::Completed),
            "turn below threshold must complete"
        );
    }

    // 5th turn: First → retry, Backoff → retry, Backoff → retry, Stop →
    // the stop-to-steering conversion retries (steering notice + steering +
    // ladder reset), up to the cap of 3; the fresh ladder re-escalates
    // First → retry, Backoff → retry, Backoff → retry, Stop → the cap is
    // exhausted and the stop at the cap is terminal: the turn ends with an
    // EarlyReturn whose response text is the exact stop text; the executor
    // surfaces it. Trace: turns 5-8 (First, Backoff, Backoff, Stop) + 3
    // conversion retry sequences of turns 9-12, 13-16, 17-20 = 20 in-stream
    // model calls.
    let repeated: Vec<Vec<MockStreamEvent>> = (0..20)
        .map(|_| {
            vec![
                MockStreamEvent::Text("I will do the thing.".to_string()),
                MockStreamEvent::final_response_with_total_tokens(1),
            ]
        })
        .collect();
    let model = MockCompletionModel::from_stream_turns(repeated);
    let (r, last_events) = h.turn("do the thing", model, tool_infra.clone()).await;

    // -- Check
    let outcome = r.map_err(|e| format!("5th turn must be Ok: {e:?}"))?;
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("5th turn must return EarlyReturn".into());
    };
    let response_text = extract_response_text_from_value(&value);
    let stop_text = format!(
        "{OUTPUT_REPETITION_STOP_PREFIX} the assistant kept repeating the same output \
         after repeated steering. The run was stopped."
    );
    assert_eq!(
        response_text, stop_text,
        "response text must be the exact stop text, got: {response_text}"
    );
    assert!(
        last_events
            .iter()
            .any(|e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if *text == stop_text)),
        "must emit a Warning with the stop text; got: {last_events:?}"
    );
    // The steering notice rides every conversion; the terminal sequence
    // converted 3 times before surfacing the stop.
    let steering_notices = last_events
        .iter()
        .filter(
            |e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text == REPETITION_STEERING_NOTICE),
        )
        .count();
    assert_eq!(
        steering_notices, 3,
        "each conversion must emit the steering notice; got: {last_events:?}"
    );

    Ok(())
}

/// Cross-turn: the 5th identical completion is a First detection —
/// `on_model_turn_finished` returns `retry_with_feedback`; the retry produces
/// different content, so the turn continues (Completed) and emits a Warning
/// with the First steering text.
#[tokio::test]
async fn journey_output_repetition_cross_turn_fifth_is_first_continue() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-cross-turn-first");
    let tool_infra = no_tools();

    // -- Exec: 4 identical text-only turns (no detection), then a 5th turn
    // whose retry produces different content so the turn completes.
    for _ in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::final_response_with_total_tokens(1),
        ]]);
        let (r, _) = h.turn("do the thing", model, tool_infra.clone()).await;
        let outcome = r.map_err(|e| format!("turn {e:?}"))?;
        assert!(
            matches!(outcome, TurnOutcome::Completed),
            "turn below threshold must complete"
        );
    }

    // 5th turn: First → retry_with_feedback, then different content → continue.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::final_response_with_total_tokens(1),
        ],
        vec![
            MockStreamEvent::Text("I changed my mind.".to_string()),
            MockStreamEvent::final_response_with_total_tokens(1),
        ],
    ]);
    let (r, last_events) = h.turn("do the thing", model, tool_infra.clone()).await;

    // -- Check
    let outcome = r.map_err(|e| format!("5th turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "5th turn must continue (Completed), not stop"
    );
    assert!(
        last_events.iter().any(
            |e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text == OUTPUT_REPETITION_MESSAGE)
        ),
        "must emit a Warning with the First steering text; got: {last_events:?}"
    );

    Ok(())
}

/// Cross-turn: a text-only completion with a tool call resets the counter, so
/// 4 identical text-only turns after a tool-call turn must not trip.
#[tokio::test]
async fn journey_output_repetition_cross_turn_tool_call_resets() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-tool-reset");
    let tool_infra = echo_tool("result_42")?;

    // -- Exec: one tool-call turn, then 4 identical text-only turns.
    // The tool-call turn needs two model calls: one to emit the tool call,
    // one after the tool result to emit the final text.
    let tool_model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_echo", serde_json::json!({})),
            MockStreamEvent::final_response_with_total_tokens(1),
        ],
        vec![
            MockStreamEvent::Text("tool done".to_string()),
            MockStreamEvent::final_response_with_total_tokens(1),
        ],
    ]);
    let (r, _) = h
        .turn("call the tool", tool_model, tool_infra.clone())
        .await;
    r.map_err(|e| format!("tool-call turn must be Ok: {e:?}"))?;

    for _ in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::final_response_with_total_tokens(1),
        ]]);
        let (r, _) = h.turn("do the thing", model, tool_infra.clone()).await;
        let outcome = r.map_err(|e| format!("text-only turn must be Ok: {e:?}"))?;
        assert!(
            matches!(outcome, TurnOutcome::Completed),
            "turn after tool-call reset must complete, not trip"
        );
    }

    Ok(())
}
