use super::*;

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
                is_skip(&result),
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
                is_skip(&result),
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
                is_skip(&result),
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
