use super::*;
use std::sync::{Arc, Mutex};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Drives the detector through `OUTPUT_REPETITION_THRESHOLD` identical
/// text-only responses, returning every detection in order.
fn drive_to_threshold(state: &Arc<Mutex<RepetitionState>>) -> Vec<Option<RepetitionDetection>> {
    let mut detections = Vec::new();
    for _ in 0..OUTPUT_REPETITION_THRESHOLD {
        detections.push(OutputRepetitionDetector::record_response(
            state,
            "I will do the thing.",
            false,
        ));
    }
    detections
}

#[test]
fn five_identical_segments_trip() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let detections = drive_to_threshold(&state);

    // -- Check
    for (i, detection) in detections.iter().enumerate() {
        if i < OUTPUT_REPETITION_THRESHOLD - 1 {
            assert!(
                detection.is_none(),
                "call {i} must not trip below threshold"
            );
        } else {
            assert_eq!(
                detection,
                &Some(RepetitionDetection::First(
                    OUTPUT_REPETITION_MESSAGE.to_string()
                )),
                "call {i} must trip with the First detection"
            );
        }
    }
}

#[test]
fn four_identical_segments_do_not_trip() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec & Check
    for i in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        let detection =
            OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
        assert!(
            detection.is_none(),
            "call {i} must not trip below threshold"
        );
    }
}

#[test]
fn differing_segment_resets() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    for _ in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        assert!(
            OutputRepetitionDetector::record_response(&state, "I will do the thing.", false)
                .is_none()
        );
    }
    // A different segment resets the counter.
    assert!(
        OutputRepetitionDetector::record_response(&state, "I changed my mind.", false).is_none()
    );
    // Now only 4 identical segments remain — must not trip.
    for i in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        let detection =
            OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
        assert!(detection.is_none(), "call {i} after reset must not trip");
    }
}

#[test]
fn tool_call_presence_resets() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    for _ in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        assert!(
            OutputRepetitionDetector::record_response(&state, "I will do the thing.", false)
                .is_none()
        );
    }
    // A response containing a tool call resets the counter.
    assert!(
        OutputRepetitionDetector::record_response(&state, "I will do the thing.", true).is_none()
    );
    // Now only 4 identical text-only segments remain — must not trip.
    for i in 0..(OUTPUT_REPETITION_THRESHOLD - 1) {
        let detection =
            OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
        assert!(
            detection.is_none(),
            "call {i} after tool-call reset must not trip"
        );
    }
}

#[test]
fn trim_and_case_differences_count_as_same_segment() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));
    let variants = [
        "I will do the thing.",
        "  I will do the thing.  ",
        "i WILL do THE thing.",
        "I will do the thing.",
        "I will do the thing.",
    ];

    // -- Exec & Check
    for (i, text) in variants.iter().enumerate() {
        let detection = OutputRepetitionDetector::record_response(&state, text, false);
        if i < OUTPUT_REPETITION_THRESHOLD - 1 {
            assert!(
                detection.is_none(),
                "call {i} must not trip below threshold"
            );
        } else {
            assert!(
                detection.is_some(),
                "call {i} must trip: trim/case differences are the same segment"
            );
        }
    }
}

#[test]
fn alternating_segments_never_trip() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec & Check
    for i in 0..(OUTPUT_REPETITION_THRESHOLD * 2) {
        let text = if i % 2 == 0 {
            "I will do the thing."
        } else {
            "I will do the other thing."
        };
        let detection = OutputRepetitionDetector::record_response(&state, text, false);
        assert!(
            detection.is_none(),
            "call {i} must not trip on alternating segments"
        );
    }
}

#[test]
fn empty_text_is_ignored() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec & Check
    for _ in 0..OUTPUT_REPETITION_THRESHOLD {
        let detection = OutputRepetitionDetector::record_response(&state, "", false);
        assert!(
            detection.is_none(),
            "empty text must be ignored with no state change"
        );
    }
    // State must be unchanged: a single real segment must not trip.
    assert!(
        OutputRepetitionDetector::record_response(&state, "I will do the thing.", false).is_none()
    );
}

#[test]
fn full_text_identical_with_trailing_sentences_trips() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec & Check
    // The full normalized text is the segment; identical full text trips.
    for i in 0..OUTPUT_REPETITION_THRESHOLD {
        let text = "I will do the thing. Different trailing sentence.";
        let detection = OutputRepetitionDetector::record_response(&state, text, false);
        if i < OUTPUT_REPETITION_THRESHOLD - 1 {
            assert!(
                detection.is_none(),
                "call {i} must not trip below threshold"
            );
        } else {
            assert!(
                detection.is_some(),
                "call {i} must trip on identical full text"
            );
        }
    }
}

#[test]
fn full_text_identical_with_newlines_trips() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec & Check
    // The full normalized text is the segment; identical full text trips.
    for i in 0..OUTPUT_REPETITION_THRESHOLD {
        let text = "I will do the thing.\n\nTrailing paragraph.";
        let detection = OutputRepetitionDetector::record_response(&state, text, false);
        if i < OUTPUT_REPETITION_THRESHOLD - 1 {
            assert!(
                detection.is_none(),
                "call {i} must not trip below threshold"
            );
        } else {
            assert!(
                detection.is_some(),
                "call {i} must trip on identical full text"
            );
        }
    }
}

#[test]
fn state_accumulates_across_calls() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    // Two separate batches of identical segments accumulate to the threshold.
    for _ in 0..2 {
        assert!(
            OutputRepetitionDetector::record_response(&state, "I will do the thing.", false)
                .is_none()
        );
    }
    for _ in 0..2 {
        assert!(
            OutputRepetitionDetector::record_response(&state, "I will do the thing.", false)
                .is_none()
        );
    }
    let detection =
        OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);

    // -- Check
    assert!(
        detection.is_some(),
        "state must accumulate across calls to reach the threshold"
    );
}

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
    assert!(
        state
            .lock()
            .map_err(|_| "should lock")?
            .recent_deltas
            .is_empty(),
        "reset_ladder must clear recent_deltas"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Intra-stream detection (record_delta)
// ---------------------------------------------------------------------------

/// Drives the detector through `OUTPUT_REPETITION_THRESHOLD` identical deltas,
/// returning every detection in order.
fn drive_deltas_to_threshold(
    state: &Arc<Mutex<RepetitionState>>,
    delta: &str,
) -> Vec<Option<RepetitionDetection>> {
    let mut detections = Vec::new();
    for _ in 0..OUTPUT_REPETITION_THRESHOLD {
        detections.push(OutputRepetitionDetector::record_delta(state, delta));
    }
    detections
}

#[test]
fn record_delta_five_identical_deltas_trip_first() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let detections = drive_deltas_to_threshold(&state, "I will do the thing.");

    // -- Check
    for (i, detection) in detections.iter().enumerate() {
        if i < OUTPUT_REPETITION_THRESHOLD - 1 {
            assert!(
                detection.is_none(),
                "delta {i} must not trip below threshold"
            );
        } else {
            assert_eq!(
                detection,
                &Some(RepetitionDetection::First(
                    OUTPUT_REPETITION_MESSAGE.to_string()
                )),
                "delta {i} must trip with the First detection"
            );
        }
    }
}

#[test]
fn record_delta_sixth_seventh_trip_backoff() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let mut detections = drive_deltas_to_threshold(&state, "I will do the thing.");
    detections.push(OutputRepetitionDetector::record_delta(
        &state,
        "I will do the thing.",
    ));
    detections.push(OutputRepetitionDetector::record_delta(
        &state,
        "I will do the thing.",
    ));

    // -- Check
    assert_eq!(
        detections[OUTPUT_REPETITION_THRESHOLD],
        Some(RepetitionDetection::Backoff(
            OUTPUT_REPETITION_BACKOFF_MESSAGE.to_string()
        )),
        "6th identical delta must be Backoff"
    );
    assert_eq!(
        detections[OUTPUT_REPETITION_THRESHOLD + 1],
        Some(RepetitionDetection::Backoff(
            OUTPUT_REPETITION_BACKOFF_MESSAGE.to_string()
        )),
        "7th identical delta must be Backoff"
    );
}

#[test]
fn record_delta_eighth_trips_stop() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let mut detections = drive_deltas_to_threshold(&state, "I will do the thing.");
    for _ in 0..3 {
        detections.push(OutputRepetitionDetector::record_delta(
            &state,
            "I will do the thing.",
        ));
    }

    // -- Check
    assert_eq!(
        detections[OUTPUT_REPETITION_THRESHOLD + 2],
        Some(RepetitionDetection::Stop(format!(
            "{OUTPUT_REPETITION_STOP_PREFIX} the assistant kept repeating the same output \
             after repeated steering. The run was stopped."
        ))),
        "8th identical delta must be Stop"
    );
}

#[test]
fn record_delta_differing_delta_requires_five_more() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    for _ in 0..3 {
        assert!(OutputRepetitionDetector::record_delta(&state, "I will do the thing.").is_none());
    }
    // A differing delta is recorded but does not trip; the sliding window is
    // not yet 5 identical.
    assert!(OutputRepetitionDetector::record_delta(&state, "I changed my mind.").is_none());
    for i in 0..4 {
        let d = OutputRepetitionDetector::record_delta(&state, "I will do the thing.");
        assert!(d.is_none(), "delta {i} after differing must not trip");
    }
    // The 5th identical after the differing one trips First.
    let d = OutputRepetitionDetector::record_delta(&state, "I will do the thing.");
    assert_eq!(
        d,
        Some(RepetitionDetection::First(
            OUTPUT_REPETITION_MESSAGE.to_string()
        )),
        "5 identical deltas after a differing one must trip First"
    );
}

#[test]
fn record_delta_empty_delta_ignored() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    for _ in 0..4 {
        assert!(OutputRepetitionDetector::record_delta(&state, "I will do the thing.").is_none());
    }
    // An empty delta is ignored (not pushed, returns None).
    assert!(OutputRepetitionDetector::record_delta(&state, "").is_none());
    // The 5th identical delta trips First — the empty delta did not count.
    let d = OutputRepetitionDetector::record_delta(&state, "I will do the thing.");
    assert_eq!(
        d,
        Some(RepetitionDetection::First(
            OUTPUT_REPETITION_MESSAGE.to_string()
        )),
        "empty delta must not count toward the window"
    );
}

#[test]
fn record_delta_cargo_response_pattern() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let detections = drive_deltas_to_threshold(&state, "cargo response");

    // -- Check
    for (i, detection) in detections.iter().enumerate() {
        if i < OUTPUT_REPETITION_THRESHOLD - 1 {
            assert!(
                detection.is_none(),
                "delta {i} must not trip below threshold"
            );
        } else {
            assert_eq!(
                detection,
                &Some(RepetitionDetection::First(
                    OUTPUT_REPETITION_MESSAGE.to_string()
                )),
                "5 identical 'cargo response' deltas must trip First"
            );
        }
    }
}
