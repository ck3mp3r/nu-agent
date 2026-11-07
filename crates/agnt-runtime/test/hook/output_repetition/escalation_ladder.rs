use super::*;

// ---------------------------------------------------------------------------
// Escalation ladder (cross-turn)
// ---------------------------------------------------------------------------

/// Drives the detector through the full escalation ladder: 5 identical
/// responses trip the First detection, then each subsequent identical response
/// escalates Backoff, Backoff, Stop.
#[test]
fn escalation_ladder_first_backoff_backoff_stop() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let mut detections = Vec::new();
    for _ in 0..8 {
        detections.push(OutputRepetitionDetector::record_response(
            &state,
            "I will do the thing.",
            false,
        ));
    }

    // -- Check
    // Calls 0-3: below threshold (None). Call 4: First. Calls 5-6: Backoff.
    // Call 7: Stop.
    for (i, d) in detections.iter().enumerate() {
        match i {
            0..=3 => assert!(d.is_none(), "call {i} must not trip below threshold"),
            4 => assert_eq!(
                d,
                &Some(RepetitionDetection::First(
                    OUTPUT_REPETITION_MESSAGE.to_string()
                )),
                "call {i} must be First"
            ),
            5 | 6 => assert_eq!(
                d,
                &Some(RepetitionDetection::Backoff(
                    OUTPUT_REPETITION_BACKOFF_MESSAGE.to_string()
                )),
                "call {i} must be Backoff"
            ),
            7 => assert_eq!(
                d,
                &Some(RepetitionDetection::Stop(format!(
                    "{OUTPUT_REPETITION_STOP_PREFIX} the assistant kept repeating the same output \
                     after repeated steering. The run was stopped."
                ))),
                "call {i} must be Stop"
            ),
            _ => unreachable!(),
        }
    }
}

/// A differing segment resets the escalation ladder, so a subsequent identical
/// run starts again at First.
#[test]
fn differing_segment_resets_escalation_ladder() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    // Reach Backoff (6 identical responses: 4 below + First + Backoff).
    for _ in 0..6 {
        OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
    }
    // A differing segment resets the ladder.
    assert!(
        OutputRepetitionDetector::record_response(&state, "I changed my mind.", false).is_none()
    );
    // 4 identical responses after the reset must not trip.
    for i in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        let d = OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
        assert!(d.is_none(), "call {i} after reset must not trip");
    }
    // The 5th identical response after the reset is First again.
    let d = OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
    assert_eq!(
        d,
        Some(RepetitionDetection::First(
            OUTPUT_REPETITION_MESSAGE.to_string()
        )),
        "after a differing-segment reset the next detection must be First"
    );
}

/// A tool-call response resets the escalation ladder, so a subsequent identical
/// run starts again at First.
#[test]
fn tool_call_resets_escalation_ladder() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    // Reach Backoff (6 identical responses: 4 below + First + Backoff).
    for _ in 0..6 {
        OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
    }
    // A tool-call response resets the ladder.
    assert!(
        OutputRepetitionDetector::record_response(&state, "I will do the thing.", true).is_none()
    );
    // 4 identical responses after the reset must not trip.
    for i in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        let d = OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
        assert!(d.is_none(), "call {i} after reset must not trip");
    }
    // The 5th identical response after the reset is First again.
    let d = OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
    assert_eq!(
        d,
        Some(RepetitionDetection::First(
            OUTPUT_REPETITION_MESSAGE.to_string()
        )),
        "after a tool-call reset the next detection must be First"
    );
}

/// `reset_ladder` clears the escalation counter while keeping the session-scoped
/// segment tracking, so a subsequent identical run starts again at First.
#[test]
fn reset_ladder_clears_escalation_counter() -> Result<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    // Reach Backoff (6 identical responses: 4 below + First + Backoff).
    for _ in 0..6 {
        OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
    }
    state.lock().map_err(|_| "should lock")?.reset_ladder();
    // The segment tracking is preserved, so the next identical response trips
    // immediately — but the ladder is reset, so it is First again, not Stop.
    let d = OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
    assert_eq!(
        d,
        Some(RepetitionDetection::First(
            OUTPUT_REPETITION_MESSAGE.to_string()
        )),
        "after a ladder reset the next detection must be First"
    );
    // The growth-throttle mark is reset too, so the next aggregated check is
    // not skipped by the throttle.
    assert_eq!(
        state.lock().map_err(|_| "should lock")?.last_check_len,
        0,
        "reset_ladder must clear last_check_len"
    );

    Ok(())
}
