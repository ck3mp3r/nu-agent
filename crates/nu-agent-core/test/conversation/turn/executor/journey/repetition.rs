use super::*;

// ---------------------------------------------------------------------------
// Doom-loop journey: 8 identical tool calls → stop on detection 4
// ---------------------------------------------------------------------------

/// A doom-loop journey drives 8 identical tool calls (5 threshold + 1 first +
/// 2 backoff + 1 stop). The turn ends with an EarlyReturn whose response text
/// starts with DOOM_LOOP_STOP_PREFIX and names the looping tool; the bus
/// carries a Warning and a Stopped notice with the same prefix; the
/// persisted history contains the skip-result ToolResults.
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
            echo_tool("result_42"),
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
        !events.iter().any(|e| matches!(e, UiEvent::AssistantMessage { text } if text.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "the stop reason must NOT ride AssistantMessage; got: {events:?}"
    );

    let msgs = h.raw_messages().await?;
    let skip_results: Vec<&Message> = msgs
        .iter()
        .filter(|m| matches!(m, Message::User { .. }))
        .collect();
    assert!(
        skip_results.iter().any(|m| {
            let Message::User { content } = m else {
                return false;
            };
            content.iter().any(|c| {
                if let crate::types::UserContent::ToolResult(tr) = c {
                    tr.content.iter().any(|rc| {
                        if let crate::types::ToolResultContent::Text(t) = rc {
                            t.text.starts_with("Doom loop detected:")
                                || t.text.starts_with("Doom loop persisted:")
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            })
        }),
        "persisted history must contain the skip-result ToolResults; got: {msgs:?}"
    );

    Ok(())
}

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
            .any(|e| matches!(e, UiEvent::Warning { message } if *message == stop_text)),
        "must emit a Warning with the stop text; got: {last_events:?}"
    );
    // The steering notice rides every conversion; the terminal sequence
    // converted 3 times before surfacing the stop.
    let steering_notices = last_events
        .iter()
        .filter(
            |e| matches!(e, UiEvent::Warning { message } if message == REPETITION_STEERING_NOTICE),
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
            |e| matches!(e, UiEvent::Warning { message } if message == OUTPUT_REPETITION_MESSAGE)
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
    let tool_infra = echo_tool("result_42");

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

/// Intra-stream: 8 identical sentences streamed within one response. The
/// aggregated text trips intra-stream detection at the 100-byte growth gate
/// (the suffix check sees 5 aligned 20-byte-period repetitions) — every
/// detection stops the stream. The executor's stop-to-steering retry
/// converts the FIRST stop: steering appended to memory + ladder reset +
/// re-run. The retry consumes a scripted distinct turn and COMPLETES.
#[tokio::test]
async fn journey_output_repetition_intra_stream_trips() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-intra-stream");
    let tool_infra = no_tools();

    // 8 identical sentences stop the stream mid-turn; the stop-to-steering
    // retry consumes the second scripted turn (different content) and
    // completes the run.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("I changed my mind.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    // -- Exec
    let (r, events) = h.turn("do the thing", model, tool_infra).await;

    // -- Check: the retry turn completed the run.
    let outcome = r.map_err(|e| format!("intra-stream trip must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "the stop-to-steering retry must complete the run, got {outcome:?}"
    );
    // The steering message was appended to memory (the conversion).
    let msgs = h.raw_messages().await?;
    assert!(
        msgs.iter()
            .any(|m| message_text(m).as_deref() == Some(OUTPUT_REPETITION_BACKOFF_MESSAGE)),
        "the conversion must append the repetition steering message; got: {msgs:?}"
    );
    // The executor publishes a warning for the conversion so the user sees
    // that steering is happening instead of only a silent retry.
    assert!(
        events.iter().any(
            |e| matches!(e, UiEvent::Warning { message } if message == REPETITION_STEERING_NOTICE)
        ),
        "the conversion retry must emit the steering notice; got: {events:?}"
    );

    Ok(())
}

/// Intra-stream: the 5th identical sentence opens the 100-byte growth gate —
/// the detection returns Stop (every detection stops); the executor converts
/// it to a steering retry which consumes a scripted distinct turn and
/// completes the run with the steering message in memory.
#[tokio::test]
async fn journey_output_repetition_intra_stream_fifth_is_first_continue() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-intra-stream-first");
    let tool_infra = no_tools();

    // 5 identical sentences stop the stream mid-turn; the stop-to-steering
    // retry consumes the second scripted turn (different content) and
    // completes the run.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("I changed my mind.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    // -- Exec
    let (r, events) = h.turn("do the thing", model, tool_infra).await;

    // -- Check: the retry turn completed the run.
    let outcome = r.map_err(|e| format!("5-sentence turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "the stop-to-steering retry must complete the run, got {outcome:?}"
    );
    // The steering message was appended to memory (the conversion).
    let msgs = h.raw_messages().await?;
    assert!(
        msgs.iter()
            .any(|m| message_text(m).as_deref() == Some(OUTPUT_REPETITION_BACKOFF_MESSAGE)),
        "the conversion must append the repetition steering message; got: {msgs:?}"
    );
    // The executor publishes a warning for the conversion so the user sees
    // that steering is happening instead of only a silent retry.
    assert!(
        events.iter().any(
            |e| matches!(e, UiEvent::Warning { message } if message == REPETITION_STEERING_NOTICE)
        ),
        "the conversion retry must emit the steering notice; got: {events:?}"
    );

    Ok(())
}

/// Intra-stream: 4 identical sentences streamed within one response do not
/// trip; the turn completes normally.
#[tokio::test]
async fn journey_output_repetition_intra_stream_four_does_not_trip() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-intra-stream-four");
    let tool_infra = no_tools();

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    // -- Exec
    let (r, _) = h.turn("do the thing", model, tool_infra).await;

    // -- Check
    let outcome = r.map_err(|e| format!("4-sentence turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "4 identical sentences must not trip intra-stream"
    );

    Ok(())
}

/// Intra-stream: alternating segments never trip, even well past the threshold.
#[tokio::test]
async fn journey_output_repetition_intra_stream_alternating_never_trips() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-intra-stream-alt");
    let tool_infra = no_tools();

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    // -- Exec
    let (r, _) = h.turn("do the thing", model, tool_infra).await;

    // -- Check
    let outcome = r.map_err(|e| format!("alternating turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "alternating segments must never trip intra-stream"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario: repetition_guard disabled — detectors are skipped entirely
// ---------------------------------------------------------------------------

/// Guard disabled, intra-stream: 8 identical sentences streamed in one
/// response would normally stop the run at the 8th delta, but with the guard
/// off the detector is never consulted — the turn completes with no warning.
#[tokio::test]
async fn journey_repetition_guard_off_intra_stream_never_trips() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-repetition-guard-off-intra-stream");
    let mut tool_infra = no_tools();
    tool_infra.repetition_guard = false;

    // 8 identical sentences — would escalate to Stop with the guard on.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    // -- Exec
    let (r, events) = h.turn("do the thing", model, tool_infra).await;

    // -- Check
    let outcome = r.map_err(|e| format!("guard-off turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "guard-off intra-stream repetition must not stop the run"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, UiEvent::Warning { message } if message.contains("repetition"))),
        "guard-off must not emit repetition warnings; got: {events:?}"
    );

    Ok(())
}

/// Guard disabled, cross-turn: 8 identical text-only completions across
/// separate `execute_turn` calls would stop the run on the 8th turn, but with
/// the guard off no detection ever fires — all turns complete.
#[tokio::test]
async fn journey_repetition_guard_off_cross_turn_never_trips() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-repetition-guard-off-cross-turn");
    let mut tool_infra = no_tools();
    tool_infra.repetition_guard = false;

    // -- Exec: 8 identical text-only turns; the 8th would Stop with guard on.
    for _ in 0..(OUTPUT_REPETITION_THRESHOLD + 3) {
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::final_response_with_total_tokens(1),
        ]]);
        let (r, events) = h.turn("do the thing", model, tool_infra.clone()).await;
        let outcome = r.map_err(|e| format!("turn {e:?}"))?;
        assert!(
            matches!(outcome, TurnOutcome::Completed),
            "guard-off cross-turn repetition must not stop the run"
        );
        assert!(
            !events.iter().any(
                |e| matches!(e, UiEvent::Warning { message } if message.contains("repetition"))
            ),
            "guard-off must not emit repetition warnings"
        );
    }

    Ok(())
}

/// Guard disabled, doom loop: 8 identical tool calls would stop the run, but
/// with the guard off the doom detector is never consulted — all calls execute
/// and the turn completes.
#[tokio::test]
async fn journey_repetition_guard_off_doom_loop_never_trips() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-repetition-guard-off-doom");
    let mut tool_infra = echo_tool("result_42");
    tool_infra.repetition_guard = false;

    // 8 identical tool calls (would stop at the 8th with the guard on) plus a
    // final text turn so the guard-off run completes normally.
    let mut turns: Vec<Vec<MockStreamEvent>> = (0..8)
        .map(|i| {
            vec![
                MockStreamEvent::tool_call(format!("tc{i}"), "test_echo", serde_json::json!({})),
                MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    turns.push(vec![
        MockStreamEvent::Text("all done".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]);
    let model = MockCompletionModel::from_stream_turns(turns);

    // -- Exec
    let (r, events) = h
        .turn("do the same thing repeatedly", model, tool_infra)
        .await;

    // -- Check
    let outcome = r.map_err(|e| format!("guard-off doom turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "guard-off doom loop must not stop the run, got: {outcome:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, UiEvent::Warning { message } if message.contains("Doom loop"))),
        "guard-off must not emit doom-loop warnings; got: {events:?}"
    );

    Ok(())
}
