use super::*;
use rig::agent::ToolCallAction;
use std::sync::{Arc, Mutex};

use crate::bus::create_bus;

type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// -- Test Support

/// Drives the detector through `DOOM_LOOP_THRESHOLD` identical calls, returning
/// every action in order. Call `i < DOOM_LOOP_THRESHOLD - 1` is under threshold;
/// the final call is the first detection.
async fn drive_to_first_detection(
    detector: &DoomLoopDetector,
    bus: &Bus,
) -> Vec<Option<ToolCallAction>> {
    let mut actions = Vec::new();
    for _ in 0..DOOM_LOOP_THRESHOLD {
        let action = detector
            .check_and_record("read_file", "{\"path\": \"same\"}", bus)
            .await;
        actions.push(action);
    }
    actions
}

#[tokio::test]
async fn no_doom_loop_under_threshold() {
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    for _ in 0..(DOOM_LOOP_THRESHOLD - 1) {
        let result = detector
            .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
            .await;
        assert!(result.is_none());
    }
}

#[tokio::test]
async fn doom_loop_fires_at_threshold() {
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    for i in 0..DOOM_LOOP_THRESHOLD {
        let result = detector
            .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
            .await;
        if i < DOOM_LOOP_THRESHOLD - 1 {
            assert!(result.is_none(), "call {i} should not trip doom loop");
        } else {
            assert!(
                matches!(result, Some(ToolCallAction::Skip { .. })),
                "call {i} should skip with detection message"
            );
        }
    }
}

#[test]
fn doom_loop_state_reset_clears_signatures() {
    let mut state = DoomLoopState::default();
    for _ in 0..4 {
        state.check_and_record("tool", "args");
    }
    assert_eq!(state.recent_signatures.len(), 4);
    state.reset();
    assert_eq!(state.recent_signatures.len(), 0);
}

#[tokio::test]
async fn different_args_does_not_trip_doom_loop() {
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    for i in 0..DOOM_LOOP_THRESHOLD {
        let result = detector
            .check_and_record("read_file", &format!("{{\"path\": \"{i}\"}}"), &bus)
            .await;
        assert!(
            result.is_none(),
            "call {i} should not trip doom loop (different args)"
        );
    }
}

#[test]
fn cross_turn_reset_prevents_false_positive() {
    let mut state = DoomLoopState::default();

    // Simulate 4 identical calls in a failed turn
    for _ in 0..4 {
        assert!(
            state
                .check_and_record("read_file", "{\"path\": \"same\"}")
                .is_none()
        );
    }
    assert_eq!(state.recent_signatures.len(), 4);

    // Turn resets at start of next turn
    state.reset();
    assert_eq!(state.recent_signatures.len(), 0);

    // Only 1 more identical call on the new turn — should NOT trip
    let result = state.check_and_record("read_file", "{\"path\": \"same\"}");
    assert!(
        result.is_none(),
        "single call after reset must not trip doom loop"
    );
}

#[test]
fn canonicalize_args_sorts_object_keys() {
    // -- Setup & Fixtures
    let reordered = r#"{"b": 2, "a": 1}"#;
    let sorted = r#"{"a":1,"b":2}"#;

    // -- Exec & Check
    assert_eq!(
        canonicalize_args(reordered),
        canonicalize_args(sorted),
        "key order must not affect canonical form"
    );
}

#[test]
fn canonicalize_args_strips_whitespace() {
    // -- Setup & Fixtures
    let spaced = r#"{ "path" : "same" }"#;
    let compact = r#"{"path":"same"}"#;

    // -- Exec & Check
    assert_eq!(
        canonicalize_args(spaced),
        canonicalize_args(compact),
        "whitespace must not affect canonical form"
    );
}

#[test]
fn canonicalize_args_keeps_raw_on_parse_failure() {
    // -- Setup & Fixtures
    let raw = "not json at all";

    // -- Exec & Check
    assert_eq!(canonicalize_args(raw), raw, "non-JSON args must stay raw");
}

#[tokio::test]
async fn reordered_json_args_trip_doom_loop() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();
    let variants = [
        r#"{"path": "same", "mode": "r"}"#,
        r#"{"mode": "r", "path": "same"}"#,
        r#"{ "path": "same", "mode": "r" }"#,
        r#"{"mode":"r","path":"same"}"#,
        r#"{"path":"same","mode":"r"}"#,
    ];

    // -- Exec & Check
    for (i, args) in variants.iter().enumerate() {
        let result = detector.check_and_record("read_file", args, &bus).await;
        if i < DOOM_LOOP_THRESHOLD - 1 {
            assert!(result.is_none(), "call {i} should not trip doom loop");
        } else {
            assert!(
                matches!(result, Some(ToolCallAction::Skip { .. })),
                "call {i} should skip with detection message"
            );
        }
    }
}

#[tokio::test]
async fn whitespace_only_args_trip_doom_loop() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();
    let variants = [
        r#"{"path":"same"}"#,
        r#"{ "path": "same" }"#,
        r#"{"path" : "same"}"#,
        r#"{"path":  "same"}"#,
        r#"{"path":"same" }"#,
    ];

    // -- Exec & Check
    for (i, args) in variants.iter().enumerate() {
        let result = detector.check_and_record("read_file", args, &bus).await;
        if i < DOOM_LOOP_THRESHOLD - 1 {
            assert!(result.is_none(), "call {i} should not trip doom loop");
        } else {
            assert!(
                matches!(result, Some(ToolCallAction::Skip { .. })),
                "call {i} should skip with detection message"
            );
        }
    }
}

#[tokio::test]
async fn non_json_args_use_raw_signature() {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    // -- Exec & Check
    for i in 0..DOOM_LOOP_THRESHOLD {
        let result = detector
            .check_and_record("read_file", "raw args", &bus)
            .await;
        if i < DOOM_LOOP_THRESHOLD - 1 {
            assert!(result.is_none(), "call {i} should not trip doom loop");
        } else {
            assert!(
                matches!(result, Some(ToolCallAction::Skip { .. })),
                "call {i} should skip with detection message"
            );
        }
    }
}

#[test]
fn canonicalize_args_sorts_nested_object_keys() {
    // -- Setup & Fixtures
    let reordered = r#"{"outer": {"z": 1, "a": {"y": 2, "b": 3}}, "mid": 4}"#;
    let sorted = r#"{"mid":4,"outer":{"a":{"b":3,"y":2},"z":1}}"#;

    // -- Exec & Check
    assert_eq!(
        canonicalize_args(reordered),
        canonicalize_args(sorted),
        "nested object keys must be sorted at every level"
    );
}

#[test]
fn canonicalize_args_preserves_array_element_order() {
    // -- Setup & Fixtures
    let forward = r#"[1,2]"#;
    let reversed = r#"[2,1]"#;

    // -- Exec & Check
    assert_ne!(
        canonicalize_args(forward),
        canonicalize_args(reversed),
        "array element order is significant"
    );
    assert_eq!(canonicalize_args(forward), "[1,2]");
}

#[test]
fn canonicalize_args_canonicalizes_array_elements() {
    // -- Setup & Fixtures
    let spaced = r#"[{"b": 2, "a": 1}, {"d": 4, "c": 3}]"#;
    let compact = r#"[{"a":1,"b":2},{"c":3,"d":4}]"#;

    // -- Exec & Check
    assert_eq!(
        canonicalize_args(spaced),
        canonicalize_args(compact),
        "array elements must be canonicalized recursively"
    );
}

#[test]
fn canonicalize_args_compact_serializes_top_level_scalars() {
    // -- Setup & Fixtures
    let cases = [
        ("5", "5"),
        ("true", "true"),
        (r#""x""#, r#""x""#),
        ("null", "null"),
        ("  5  ", "5"),
    ];

    // -- Exec & Check
    for (raw, expected) in cases {
        assert_eq!(
            canonicalize_args(raw),
            expected,
            "scalar {raw} must re-serialize compactly"
        );
    }
}

#[test]
fn canonicalize_args_uses_last_duplicate_key_value() {
    // -- Setup & Fixtures
    let duplicated = r#"{"path": "first", "path": "last"}"#;
    let expected = r#"{"path":"last"}"#;

    // -- Exec & Check
    assert_eq!(
        canonicalize_args(duplicated),
        expected,
        "duplicate keys must keep the last occurrence's value"
    );
}

/// The first detection in a turn attempt skips with a message that both
/// reports the detection fact and challenges the model to reconsider.
#[tokio::test]
async fn first_detection_skips_with_steering_message() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();
    let pinned = [
        "Doom loop detected: 'read_file' called 5 times with identical arguments",
        "Are you really sure you need so many tool calls?",
        "Reconsider your approach",
    ];

    // -- Exec & Check
    let actions = drive_to_first_detection(&detector, &bus).await;
    let last = actions
        .last()
        .ok_or("should have at least one action")?
        .clone()
        .ok_or("threshold call should produce an action")?;
    match last {
        ToolCallAction::Skip(message) => {
            for text in &pinned {
                assert!(
                    message.contains(text),
                    "first-detection skip message must contain the pinned steering text {text:?}, got: {message}"
                );
            }
        }
        other => return Err(format!("should be Skip, got: {other:?}").into()),
    }
    Ok(())
}

/// The second detection in the same turn attempt is a backoff: skip with
/// stronger steering that names the looping tool.
#[tokio::test]
async fn second_detection_backoff_skips_naming_tool() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    // -- Exec & Check
    let actions = drive_to_first_detection(&detector, &bus).await;
    assert!(
        matches!(actions.last(), Some(Some(ToolCallAction::Skip { .. }))),
        "first detection should skip with steering"
    );

    let second = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    match second {
        Some(ToolCallAction::Skip(message)) => {
            assert!(
                message.starts_with("Doom loop persisted:"),
                "backoff message must start with 'Doom loop persisted:', got: {message}"
            );
            assert!(
                message.contains("read_file"),
                "backoff message must name the looping tool, got: {message}"
            );
            assert!(
                message.contains("Change your approach"),
                "backoff message must contain the pinned steering text, got: {message}"
            );
        }
        other => return Err(format!("should be Skip, got: {other:?}").into()),
    }
    Ok(())
}

/// Detections 2 and 3 in the same turn attempt are backoffs (skip with
/// stronger steering); detection 4 stops the run.
#[tokio::test]
async fn fourth_detection_stops_run() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    // -- Exec & Check
    let _ = drive_to_first_detection(&detector, &bus).await;
    let second = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(
        matches!(second, Some(ToolCallAction::Skip { .. })),
        "second detection should be a backoff skip"
    );
    let third = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(
        matches!(third, Some(ToolCallAction::Skip { .. })),
        "third detection should be a backoff skip"
    );
    let fourth = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    match fourth {
        Some(ToolCallAction::Stop(message)) => {
            assert!(
                message.starts_with(DOOM_LOOP_STOP_PREFIX),
                "stop message must start with DOOM_LOOP_STOP_PREFIX, got: {message}"
            );
            assert!(
                message.contains("read_file"),
                "stop message must name the looping tool, got: {message}"
            );
            assert!(
                message.contains("stopped"),
                "stop message must state the run was stopped, got: {message}"
            );
        }
        other => return Err(format!("should be Stop, got: {other:?}").into()),
    }
    Ok(())
}

/// The stop detection returns the stop action and emits no warning on the bus
/// warning channel — the executor surfaces the stop reason exactly once.
#[tokio::test]
async fn stop_detection_emits_no_warning() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();
    let mut warning_rx = bus.warning().subscribe();

    // -- Exec
    // First detection (skip + warning), then drain the warning channel.
    let _ = drive_to_first_detection(&detector, &bus).await;
    let _ = warning_rx.try_recv();
    // Second detection (backoff skip + warning), then drain.
    let second = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(
        matches!(second, Some(ToolCallAction::Skip { .. })),
        "second detection should be a backoff skip"
    );
    let _ = warning_rx.try_recv();
    // Third detection (backoff skip + warning), then drain.
    let third = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(
        matches!(third, Some(ToolCallAction::Skip { .. })),
        "third detection should be a backoff skip"
    );
    let _ = warning_rx.try_recv();
    // Fourth detection (stop) — must emit no warning.
    let fourth = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;

    // -- Check
    assert!(
        matches!(fourth, Some(ToolCallAction::Stop(_))),
        "fourth detection should stop the run"
    );
    assert!(
        matches!(warning_rx.try_recv(), Err(crate::bus::TryRecvError::Empty)),
        "stop detection must not emit a warning on the bus warning channel"
    );
    Ok(())
}

/// After reset, a detection is a first detection again: skip with steering,
/// not stop.
#[tokio::test]
async fn reset_clears_escalation_counter() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    // -- Exec & Check
    let _ = drive_to_first_detection(&detector, &bus).await;
    let second = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(
        matches!(second, Some(ToolCallAction::Skip { .. })),
        "second detection should be a backoff skip before reset"
    );

    state.lock().map_err(|_| "should lock")?.reset();

    let actions = drive_to_first_detection(&detector, &bus).await;
    assert!(
        matches!(actions.last(), Some(Some(ToolCallAction::Skip { .. }))),
        "detection after reset should be a first detection again (skip)"
    );
    Ok(())
}
