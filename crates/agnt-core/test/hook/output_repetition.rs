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
    // The growth-throttle mark is reset too, so the next aggregated check is
    // not skipped by the throttle.
    assert_eq!(
        state.lock().map_err(|_| "should lock")?.last_check_len,
        0,
        "reset_ladder must clear last_check_len"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Intra-stream detection (check_aggregated)
// ---------------------------------------------------------------------------

/// Repeats `unit` `count` times joined with `sep`, plus the separator before
/// the first unit (matching how live streams emit `sep` between units).
fn repeat_separated(unit: &str, sep: &str, count: usize) -> String {
    let mut text = String::new();
    for i in 0..count {
        if i > 0 {
            text.push_str(sep);
        }
        text.push_str(unit);
    }
    text
}

/// Regression: live-failed pattern 1 — "apple banana cherry date elderberry"
/// repeated 9 times separated by newlines was NOT caught by the previous detector.
#[test]
fn check_aggregated_apple_banana_pattern_trips() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let detected = OutputRepetitionDetector::check_aggregated(
        &state,
        &repeat_separated("apple banana cherry date elderberry", "\n", 9),
    );

    // -- Check
    assert_eq!(
        detected,
        Some(RepetitionAction::Stop),
        "word-list repetition with newline separators must be detected"
    );
}

/// Regression: live-failed pattern 2 — "TheeaglehaslandedNushellisnotbash"
/// repeated 13 times (no word boundaries) was NOT caught by the previous detector.
#[test]
fn check_aggregated_concatenated_phrase_trips() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let detected = OutputRepetitionDetector::check_aggregated(
        &state,
        &"TheeaglehaslandedNushellisnotbash".repeat(13),
    );

    // -- Check
    assert_eq!(
        detected,
        Some(RepetitionAction::Stop),
        "boundaryless concatenated phrase repetition must be detected"
    );
}

/// Regression: live-failed pattern 3 — "The eagle has landed. Nushell is not
/// bash." repeated 13 times (varying delta splits) was NOT caught by the
/// previous detector.
#[test]
fn check_aggregated_punctuated_phrase_trips() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let detected = OutputRepetitionDetector::check_aggregated(
        &state,
        &"The eagle has landed. Nushell is not bash. ".repeat(13),
    );

    // -- Check
    assert_eq!(
        detected,
        Some(RepetitionAction::Stop),
        "punctuated phrase repetition must be detected"
    );
}

/// Non-repetitive realistic multi-sentence text must not trip.
#[test]
fn check_aggregated_non_repetitive_returns_false() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let detected = OutputRepetitionDetector::check_aggregated(
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
    assert_eq!(detected, None, "non-repetitive text must not be detected");
}

/// Below threshold (4 repetitions of a phrase) must not trip.
#[test]
fn check_aggregated_below_threshold_returns_false() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let detected = OutputRepetitionDetector::check_aggregated(
        &state,
        "Hello world. Hello world. Hello world. Hello world.",
    );

    // -- Check
    assert_eq!(
        detected, None,
        "4 repetitions must not trip (threshold is {OUTPUT_REPETITION_THRESHOLD})"
    );
}

/// The growth throttle must skip checks until the aggregated text grows by
/// `OUTPUT_REPETITION_CHECK_INTERVAL` bytes, and must not update the mark when
/// the gate stays closed.
#[test]
fn check_aggregated_growth_throttle_skips_until_gate_opens() -> Result<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));
    let repetitive = "TheeaglehaslandedNushellisnotbash".repeat(13);

    // -- Exec & Check
    // First call: below 100 chars of aggregated → skipped without checking and
    // the mark stays 0.
    let short = &repetitive[..40];
    assert_eq!(
        OutputRepetitionDetector::check_aggregated(&state, short),
        None,
        "aggregated shorter than the check interval must be skipped"
    );
    assert_eq!(
        state.lock().map_err(|_| "should lock")?.last_check_len,
        0,
        "the throttle mark must not update when the gate stays closed"
    );

    // Second call: still under 100 chars total growth → skipped again.
    let short2 = &repetitive[..80];
    assert_eq!(
        OutputRepetitionDetector::check_aggregated(&state, short2),
        None,
        "aggregated growth under the interval must still be skipped"
    );
    assert_eq!(
        state.lock().map_err(|_| "should lock")?.last_check_len,
        0,
        "the mark must not advance on skipped checks"
    );

    // Third call: past 100 chars → the gate opens (mark advances) and the
    // repetitive suffix is detected.
    let detected = OutputRepetitionDetector::check_aggregated(&state, &repetitive);
    assert_eq!(
        detected,
        Some(RepetitionAction::Stop),
        "aggregated growth past the interval must open the gate and detect"
    );

    Ok(())
}

/// Units shorter than [`MIN_UNIT`] (8) bytes repeated >= threshold times must
/// NOT be detected (ordinary short-byte repetition is not a doom-loop signal);
/// an exactly-8-byte unit repeated 5 times IS detected.
#[test]
fn check_aggregated_short_unit_ignored() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec & Check
    // "a" x40 (single-byte unit, repeated well past the threshold).
    assert_eq!(
        OutputRepetitionDetector::check_aggregated(&state, &"a".repeat(40)),
        None,
        "1-byte unit repetition must be ignored (MIN_UNIT floor)"
    );
    // "ab" x20 — still under the floor.
    assert_eq!(
        OutputRepetitionDetector::check_aggregated(&state, &"ab".repeat(20)),
        None,
        "2-byte unit repetition must be ignored (MIN_UNIT floor)"
    );
    // An 8-byte unit: 13 reps (104 bytes) open the 100-char growth gate, and
    // the suffix of 5 aligned 8-byte units is detected.
    assert_eq!(
        OutputRepetitionDetector::check_aggregated(&state, &"abcdefgh".repeat(13)),
        Some(RepetitionAction::Stop),
        "8-byte unit repeated past the threshold must be detected"
    );
}

/// `check_aggregated` is detect-only: repeated detections must leave the
/// escalation counter untouched — the ladder is consumed exclusively by
/// `record_response` (cross-turn) or `finish_turn` (turn-boundary flag path).
#[test]
fn check_aggregated_does_not_escalate() -> Result<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));
    let unit = "I will do the thing. ";

    // -- Exec & Check
    // Several gate-open detections (growth > interval between calls).
    assert_eq!(
        OutputRepetitionDetector::check_aggregated(&state, &unit.repeat(8)),
        Some(RepetitionAction::Stop)
    );
    assert_eq!(
        OutputRepetitionDetector::check_aggregated(&state, &unit.repeat(16)),
        Some(RepetitionAction::Stop)
    );
    assert_eq!(
        OutputRepetitionDetector::check_aggregated(&state, &unit.repeat(24)),
        Some(RepetitionAction::Stop)
    );
    assert_eq!(
        OutputRepetitionDetector::check_aggregated(&state, &unit.repeat(32)),
        Some(RepetitionAction::Stop)
    );

    let escalated = state.lock().map_err(|_| "should lock")?.escalation_count;
    assert_eq!(
        escalated, 0,
        "check_aggregated must never consume the escalation ladder"
    );

    Ok(())
}

/// The suffix check must not panic on multibyte (non-ASCII) input; a CJK
/// phrase repeated past the threshold is detected like any other unit.
#[test]
fn check_aggregated_multibyte_repetition_trips() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(RepetitionState::default()));

    // -- Exec
    let detected = OutputRepetitionDetector::check_aggregated(&state, &"你好世界再見".repeat(13));

    // -- Check
    assert_eq!(
        detected,
        Some(RepetitionAction::Stop),
        "multibyte repetition must be detected without panicking"
    );
}

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

/// Drives the ladder to `escalation_count == target` through cross-turn
/// `finish_turn` escalations. Identical full-text turns accumulate
/// `consecutive`; every `threshold`th identical turn completion escalates
/// the ladder one step — so `target` escalations require
/// `threshold * target` identical turn completions.
fn drive_escalation_to(state: &Arc<Mutex<RepetitionState>>, target: usize) -> Result<()> {
    let content = [rig::message::AssistantContent::text("I will do the thing.")];
    // Identical full-text turns accumulate `consecutive`; the ladder
    // escalates once each time `consecutive` reaches a multiple of the
    // threshold. `target` escalations therefore need `threshold * target`
    // identical turn completions.
    let total = OUTPUT_REPETITION_THRESHOLD * target;
    for _ in 0..total {
        let _detection = OutputRepetitionDetector::finish_turn(state, content.as_slice());
    }
    let count = state.lock().map_err(|_| "should lock")?.escalation_count;
    assert!(
        count >= target,
        "fixture must reach at least the target escalation (got {count})"
    );
    Ok(())
}

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
