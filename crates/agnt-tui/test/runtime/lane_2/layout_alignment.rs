use super::*;

/// The lane-2 line formats the token thresholds, is right-aligned in a wide
/// layout, and uses deterministic right-anchored truncation at a narrow width.
#[test]
fn lane_2_format_and_alignment() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(200_000));

    // -- Exec & Check: the threshold formatting.
    let cases: [(u64, &str); 3] = [
        (100, "100 (0%)"),
        (1_000, "1k (0%)"),
        (11_657, "11.6k (5%)"),
    ];
    for (tokens, expected) in cases {
        state.status.tokens.latest_total_tokens = Some(tokens);
        let line = crate::runtime::lane_2_status_line_for_test(&state, 40);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.ends_with(expected), "tokens={tokens}");
    }

    // -- Setup & Fixtures: a wide layout.
    let mut wide = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(11_657),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    wide.status
        .tokens
        .set_context_window_max_tokens(Some(200_000));

    // -- Exec & Check: right-aligned in a wide layout.
    let width = 40usize;
    let line = crate::runtime::lane_2_status_line_for_test(&wide, width);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text.chars().count(), width);
    assert!(text.ends_with("11.6k (5%)"));
    assert!(text.starts_with(" "));

    // -- Exec & Check: right-anchored truncation at a narrow width.
    let line = crate::runtime::lane_2_status_line_for_test(&wide, 8);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "... (5%)");
    Ok(())
}

/// The lane-2 line shows the agent name when active and only the tokens when
/// no agent is set.
#[test]
fn lane_2_content_agent_and_tokens() -> Result<()> {
    // -- Setup & Fixtures: an active agent.
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(42_300),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128_000));
    state.set_active_agent_identity("coder");

    // -- Exec & Check: the agent name is present, the old prefix is gone.
    let line = crate::runtime::lane_2_status_line_for_test(&state, 60);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("coder"));
    assert!(!text.contains("agent:"));
    assert!(text.ends_with("42.3k (33%)"));

    // -- Setup & Fixtures: no agent.
    let mut plain = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(250),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    plain
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));

    // -- Exec & Check: only the tokens are shown.
    let line = crate::runtime::lane_2_status_line_for_test(&plain, 40);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.ends_with("250 (25%)"));
    assert!(!text.contains("agent"));
    Ok(())
}
