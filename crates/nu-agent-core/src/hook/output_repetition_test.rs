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
fn whitespace_and_case_differences_count_as_same_segment() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));
    let variants = [
        "I will do the thing.",
        "  I   will do the thing.  ",
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
                "call {i} must trip: whitespace/case differences are the same segment"
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
fn segmentation_uses_first_non_empty_sentence() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec & Check
    // The first sentence is the segment; trailing sentences are ignored.
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
                "call {i} must trip on the first sentence segment"
            );
        }
    }
}

#[test]
fn newline_separated_segments_are_split() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec & Check
    // A newline splits segments; the first non-empty is the segment.
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
                "call {i} must trip on the newline-split segment"
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

/// `escalate` stores the steering text in `pending_steering` for the First and
/// Backoff phases, and `take_pending_steering` consumes it exactly once. The
/// Stop phase does not set it.
#[test]
fn pending_steering_set_on_first_backoff_consumed_once_not_on_stop() -> Result<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec: drive to First (5th response).
    for _ in 0..OUTPUT_REPETITION_THRESHOLD {
        OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
    }

    // -- Check: First sets pending steering.
    let first_steering = state
        .lock()
        .map_err(|_| "should lock")?
        .take_pending_steering();
    assert_eq!(
        first_steering.as_deref(),
        Some(OUTPUT_REPETITION_MESSAGE),
        "First must set pending_steering to the First message"
    );
    // Consumed exactly once — a second take returns None.
    assert!(
        state
            .lock()
            .map_err(|_| "should lock")?
            .take_pending_steering()
            .is_none(),
        "pending_steering must be None after consumption"
    );

    // -- Exec: drive to Backoff (6th and 7th responses).
    for _ in 0..2 {
        OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);
    }

    // -- Check: Backoff sets pending steering.
    let backoff_steering = state
        .lock()
        .map_err(|_| "should lock")?
        .take_pending_steering();
    assert_eq!(
        backoff_steering.as_deref(),
        Some(OUTPUT_REPETITION_BACKOFF_MESSAGE),
        "Backoff must set pending_steering to the Backoff message"
    );

    // -- Exec: drive to Stop (8th response).
    OutputRepetitionDetector::record_response(&state, "I will do the thing.", false);

    // -- Check: Stop does not set pending steering.
    assert!(
        state
            .lock()
            .map_err(|_| "should lock")?
            .take_pending_steering()
            .is_none(),
        "Stop must not set pending_steering"
    );

    Ok(())
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

    Ok(())
}

// ---------------------------------------------------------------------------
// Intra-stream detection (check_streaming)
// ---------------------------------------------------------------------------

#[test]
fn check_streaming_five_identical_sentences_trips() {
    // -- Setup & Fixtures
    let aggregated = "I will do the thing. I will do the thing. I will do the thing. \
                      I will do the thing. I will do the thing.";

    // -- Exec
    let detection = OutputRepetitionDetector::check_streaming(aggregated);

    // -- Check
    assert!(detection, "5 consecutive identical sentences must trip");
}

#[test]
fn check_streaming_four_identical_sentences_do_not_trip() {
    // -- Setup & Fixtures
    let aggregated = "I will do the thing. I will do the thing. I will do the thing. \
                      I will do the thing.";

    // -- Exec
    let detection = OutputRepetitionDetector::check_streaming(aggregated);

    // -- Check
    assert!(!detection, "4 identical sentences must not trip");
}

#[test]
fn check_streaming_alternating_never_trips() {
    // -- Setup & Fixtures
    let aggregated = "I will do the thing. I will do the other thing. I will do the thing. \
                      I will do the other thing. I will do the thing. I will do the other thing. \
                      I will do the thing. I will do the other thing. I will do the thing. \
                      I will do the other thing.";

    // -- Exec
    let detection = OutputRepetitionDetector::check_streaming(aggregated);

    // -- Check
    assert!(!detection, "alternating segments must never trip");
}

#[test]
fn check_streaming_run_broken_by_differing_segment_resets() {
    // -- Setup & Fixtures
    // 4 identical, then a differing segment, then 4 more identical — the run
    // resets at the differing segment so the trailing run is 4.
    let aggregated = "I will do the thing. I will do the thing. I will do the thing. \
                      I will do the thing. I changed my mind. I will do the thing. \
                      I will do the thing. I will do the thing. I will do the thing.";

    // -- Exec
    let detection = OutputRepetitionDetector::check_streaming(aggregated);

    // -- Check
    assert!(
        !detection,
        "a differing segment must reset the trailing run"
    );
}

#[test]
fn check_streaming_newline_separated_repeats_trip() {
    // -- Setup & Fixtures
    let aggregated = "I will do the thing.\nI will do the thing.\nI will do the thing.\n\
                      I will do the thing.\nI will do the thing.";

    // -- Exec
    let detection = OutputRepetitionDetector::check_streaming(aggregated);

    // -- Check
    assert!(detection, "newline-separated repeats must trip");
}

#[test]
fn check_streaming_empty_text_returns_false() {
    // -- Setup & Fixtures
    let aggregated = "";

    // -- Exec
    let detection = OutputRepetitionDetector::check_streaming(aggregated);

    // -- Check
    assert!(!detection, "empty text must return false");
}

/// A trailing run that ends at the last complete segment trips, but a run that
/// was broken earlier and is not the trailing run does not re-fire.
#[test]
fn check_streaming_trailing_run_semantics() {
    // -- Setup & Fixtures
    // 5 identical, then a differing segment, then 4 identical — the trailing
    // run is 4, so it must not trip even though an earlier run reached 5.
    let broken_then_short = "I will do the thing. I will do the thing. I will do the thing. \
                             I will do the thing. I will do the thing. I changed my mind. \
                             I will do the thing. I will do the thing. I will do the thing. \
                             I will do the thing.";
    // 4 identical, then a differing segment, then 5 identical — the trailing
    // run is 5, so it must trip.
    let short_then_five = "I will do the thing. I will do the thing. I will do the thing. \
                           I will do the thing. I changed my mind. I will do the thing. \
                           I will do the thing. I will do the thing. I will do the thing. \
                           I will do the thing.";

    // -- Exec
    let broken = OutputRepetitionDetector::check_streaming(broken_then_short);
    let trailing = OutputRepetitionDetector::check_streaming(short_then_five);

    // -- Check
    assert!(
        !broken,
        "a broken run followed by a short trailing run must not trip"
    );
    assert!(
        trailing,
        "a short run followed by a 5-long trailing run must trip"
    );
}
