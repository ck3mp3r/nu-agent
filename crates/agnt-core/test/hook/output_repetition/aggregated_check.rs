use super::*;

// ---------------------------------------------------------------------------
// Intra-stream detection (check_aggregated)
// ---------------------------------------------------------------------------

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
