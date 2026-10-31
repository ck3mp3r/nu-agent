use super::*;

/// The lane-2 context line uses the exact usage format with no extra text, and
/// falls back to the used-only value when the max is unavailable.
#[test]
fn lane_2_context_line_format_and_fallback() -> Result<()> {
    // -- Setup & Fixtures: a known max.
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(250),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));

    // -- Exec & Check: the exact right-aligned format.
    let line = crate::runtime::lane_2_status_line_for_test(&state, 120);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(
        text,
        "                                                                                                               250 (25%)"
    );
    assert!(!text.contains("Context"));
    assert!(!text.contains("Ctrl-P"));
    assert!(!text.contains('|'));

    // -- Setup & Fixtures: no max.
    let mut fallback = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(42),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    fallback.status.tokens.set_context_window_max_tokens(None);

    // -- Exec & Check: the used-only fallback.
    let line = crate::runtime::lane_2_status_line_for_test(&fallback, 120);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(
        text,
        "                                                                                                                      42"
    );
    assert!(!text.contains("Context"));
    assert!(!text.contains("Ctrl-P"));
    assert!(!text.contains('|'));
    Ok(())
}

/// The footer exposes lane 1 and lane 2 simultaneously; the configured path
/// resolves the context max without a fallback format; the line updates after
/// each turn; and truncation removes any extra labels or hints.
#[test]
fn lane_2_context_line_two_lane_and_updates() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(250),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));

    // -- Exec & Check: lane 1 and lane 2 are exposed simultaneously.
    let lane_1 = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let lane_1_text: String = lane_1.spans.iter().map(|s| s.content.as_ref()).collect();
    let lane_2 = crate::runtime::lane_2_status_line_for_test(&state, 120);
    let lane_2_text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(lane_1_text.starts_with("○ openai/gpt-4o-mini"));
    assert!(!lane_1_text.contains('|'));
    assert!(lane_2_text.ends_with("250 (25%)"));

    // -- Setup & Fixtures: a configured max with a live completion.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128_000));
    coordinator.enqueue_event(llm_completed(40, 0, 2_500, 500, Some(3_000), None, false));
    coordinator.drain_transport();

    // -- Exec & Check: the configured path resolves the max without a fallback.
    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let lane_2_text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(lane_2_text.ends_with("3k (2%)"));
    assert!(!lane_2_text.contains('/'));

    // -- Setup & Fixtures: a small max for the update test.
    let mut updating = RuntimeCoordinator::new(120, 30, Some(true));
    updating
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(100));
    updating.enqueue_event(llm_completed(12, 0, 2, 8, Some(10), None, false));
    updating.drain_transport();

    // -- Exec & Check: the first turn.
    let first = crate::runtime::lane_2_status_line_for_test(updating.state(), 120);
    let first_text: String = first.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(first_text.ends_with("10 (10%)"));

    // -- Exec & Check: the second turn does not stale.
    updating.enqueue_event(llm_completed(20, 0, 8, 32, Some(40), None, false));
    updating.drain_transport();
    let second = crate::runtime::lane_2_status_line_for_test(updating.state(), 120);
    let second_text: String = second.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(second_text.ends_with("40 (40%)"));

    // -- Setup & Fixtures: a narrow width.
    let mut narrow = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(12345),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    narrow
        .status
        .tokens
        .set_context_window_max_tokens(Some(128000));

    // -- Exec & Check: truncation removes extra labels or hints.
    let line = crate::runtime::lane_2_status_line_for_test(&narrow, 30);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "                    12.3k (9%)");
    assert!(!text.contains("Context"));
    assert!(!text.contains("Ctrl-P"));
    assert!(!text.contains('|'));
    Ok(())
}
