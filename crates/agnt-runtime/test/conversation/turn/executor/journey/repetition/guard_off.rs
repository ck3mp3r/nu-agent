use super::*;

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
        !events.iter().any(
            |e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text.contains("repetition"))
        ),
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
                |e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text.contains("repetition"))
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
    let mut tool_infra = echo_tool("result_42")?;
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
        !events.iter().any(
            |e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text.contains("Doom loop"))
        ),
        "guard-off must not emit doom-loop warnings; got: {events:?}"
    );

    Ok(())
}
