use super::*;

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
