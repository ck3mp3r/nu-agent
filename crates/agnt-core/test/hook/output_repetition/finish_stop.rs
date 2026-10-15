use super::*;

// ---------------------------------------------------------------------------
// Turn-boundary flag consumption (finish_turn)
// ---------------------------------------------------------------------------

// With `check_aggregated` always returning Stop (and never setting the
// intra-stream flag), the turn-boundary `finish_turn` flag path is
// unreachable from live streams — the flag remains a defensive path for the
// cross-turn ladder (`record_turn_content` → `record_response`).

/// With NO intra-stream detection, `finish_turn` delegates to
/// `record_turn_content` (the cross-turn path): identical full texts
/// accumulate the consecutive counter and the ladder fires only at the
/// cross-turn threshold.
#[test]
fn finish_turn_delegates_to_cross_turn_path_without_flag() -> Result<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));
    let content = [rig::message::AssistantContent::text("I will do the thing.")];

    // -- Exec & Check
    // 4 identical turns stay below the cross-turn threshold.
    for i in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        let detection = OutputRepetitionDetector::finish_turn(&state, content.as_slice());
        assert!(
            detection.is_none(),
            "no-flag turn {i} must delegate and stay below threshold"
        );
    }
    // The 5th identical turn is the cross-turn First detection.
    let detection = OutputRepetitionDetector::finish_turn(&state, content.as_slice());
    assert_eq!(
        detection,
        Some(RepetitionDetection::First(
            OUTPUT_REPETITION_MESSAGE.to_string()
        )),
        "the 5th no-flag turn must trip the cross-turn First detection"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Intra-stream action ladder (check_aggregated → RepetitionAction)
// ---------------------------------------------------------------------------

/// WHEN `check_aggregated` detects repetition, THE `check_aggregated` RETURN
/// VALUE IS `Some(RepetitionAction::Stop)` — at ANY ladder level (the stream
/// stops on the FIRST detection; escalation happens through the
/// stop-to-steering retry, not through `check_aggregated`).
#[test]
fn check_aggregated_detection_always_returns_stop() -> Result<()> {
    // -- Setup & Fixtures: fresh state (count 0) AND a driven ladder (counts
    // 1..=4) must all yield Stop on detection.
    for pre_escalations in 0..=4usize {
        let state = Arc::new(Mutex::new(RepetitionState::default()));
        if pre_escalations > 0 {
            drive_escalation_to(&state, pre_escalations)?;
        }

        // -- Exec
        let action =
            OutputRepetitionDetector::check_aggregated(&state, &"I will do the thing. ".repeat(13));

        // -- Check
        assert_eq!(
            action,
            Some(RepetitionAction::Stop),
            "detection at escalation_count {pre_escalations} must return Stop"
        );
    }

    Ok(())
}

/// WHEN `check_aggregated` detects no repetition, THE RETURN VALUE IS `None`,
/// even after ladder escalations.
#[test]
fn check_aggregated_non_repetitive_at_stop_level_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));
    drive_escalation_to(&state, 4)?;

    // -- Exec
    let action = OutputRepetitionDetector::check_aggregated(
        &state,
        "Let me check the file contents. The file has 42 lines. I see a function \
         called main. Let me also check the tests. The build passes now. I will \
         summarize the findings next. There are three modules involved here. \
         Each module owns a distinct concern. One handles parsing of input. \
         Another renders the output for display. The last one persists state. \
         That covers the full surface area. I can proceed with confidence. \
         The plan is clear enough to follow. Next I will write the change. \
         Then a review pass happens after that. It should be quick to verify.",
    );

    // -- Check
    assert_eq!(
        action, None,
        "non-repetitive text must return None even at Stop level"
    );

    Ok(())
}

/// `check_aggregated` must not increment `escalation_count`, even when it
/// returns `Stop`: the ladder moves only at `finish_turn` and the executor's
/// stop-to-steering reset.
#[test]
fn check_aggregated_stop_does_not_escalate() -> Result<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));
    drive_escalation_to(&state, 4)?;
    let before = state.lock().map_err(|_| "should lock")?.escalation_count;

    // -- Exec
    let action =
        OutputRepetitionDetector::check_aggregated(&state, &"I will do the thing. ".repeat(13));
    assert_eq!(action, Some(RepetitionAction::Stop));

    // -- Check
    let count = state.lock().map_err(|_| "should lock")?.escalation_count;
    assert_eq!(
        count, before,
        "check_aggregated must leave the ladder untouched"
    );

    Ok(())
}
