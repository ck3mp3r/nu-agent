use super::*;

#[test]
fn lane_2_context_line_uses_exact_usage_format_without_extra_text() {
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

    let line = crate::runtime::lane_2_status_line_for_test(&state, 120);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(
        text,
        "                                                                                                               250 (25%)"
    );
    assert!(!text.contains("Context"));
    assert!(!text.contains("Ctrl-P"));
    assert!(!text.contains('|'));
}

#[test]
fn lane_2_context_line_falls_back_to_used_only_when_max_unavailable() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(42),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state.status.tokens.set_context_window_max_tokens(None);

    let line = crate::runtime::lane_2_status_line_for_test(&state, 120);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(
        text,
        "                                                                                                                      42"
    );
    assert!(!text.contains("Context"));
    assert!(!text.contains("Ctrl-P"));
    assert!(!text.contains('|'));
}

#[test]
fn footer_two_lane_contract_exposes_lane_1_and_lane_2_simultaneously() {
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

    let lane_1 = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let lane_1_text: String = lane_1.spans.iter().map(|s| s.content.as_ref()).collect();
    let lane_2 = crate::runtime::lane_2_status_line_for_test(&state, 120);
    let lane_2_text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(lane_1_text.starts_with("○ openai/gpt-4o-mini"));
    assert!(!lane_1_text.contains('|'));
    assert!(lane_2_text.ends_with("250 (25%)"));
}

#[test]
fn configured_path_resolves_context_max_without_fallback_format() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128_000));
    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 40,
        tool_calls: 0,
        input_tokens: 2_500,
        output_tokens: 500,
        total_tokens: 3_000,
    });
    coordinator.drain_transport();

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let lane_2_text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(lane_2_text.ends_with("3k (2%)"));
    assert!(!lane_2_text.contains('/'));
}

#[test]
fn lane_2_context_line_updates_after_each_turn_and_does_not_stale() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(100));

    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 12,
        tool_calls: 0,
        input_tokens: 2,
        output_tokens: 8,
        total_tokens: 10,
    });
    coordinator.drain_transport();
    let first = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let first_text: String = first.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(first_text.ends_with("10 (10%)"));

    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 20,
        tool_calls: 0,
        input_tokens: 8,
        output_tokens: 32,
        total_tokens: 40,
    });
    coordinator.drain_transport();
    let second = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let second_text: String = second.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(second_text.ends_with("40 (40%)"));
}

#[test]
fn lane_2_context_line_truncation_removes_any_extra_labels_or_hints() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(12345),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128000));

    let line = crate::runtime::lane_2_status_line_for_test(&state, 30);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(text, "                    12.3k (9%)");
    assert!(!text.contains("Context"));
    assert!(!text.contains("Ctrl-P"));
    assert!(!text.contains('|'));
}

#[test]
fn lane_2_rehydrates_used_tokens_from_hydrated_history_metadata() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("user", "hello"), {
            let mut s = UiMessageSnapshot::new("assistant", "history");
            s.usage = Some(UiMessageUsageSnapshot {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(444),
            });
            s
        }],
        None,
    );

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text.chars().count(), 120);
    assert!(text.ends_with("444"));
}

#[test]
fn lane_2_rehydrate_with_known_max_shows_ratio_immediately() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));
    coordinator.hydrate_transcript_from_messages(
        vec![{
            let mut s = UiMessageSnapshot::new("assistant", "history");
            s.usage = Some(UiMessageUsageSnapshot {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(250),
            });
            s
        }],
        None,
    );

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.ends_with("250 (25%)"));
}

#[test]
fn lane_2_rehydrate_without_usage_metadata_and_without_max_uses_fallback() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "history")],
        None,
    );

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text.chars().count(), 120);
    assert!(text.ends_with("0"));
}

#[test]
fn lane_2_rehydrate_without_usage_metadata_with_known_max_shows_ratio_not_fallback() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(100));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "history")],
        None,
    );

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.ends_with("0 (0%)"));
}

#[test]
fn lane_2_rehydrate_is_replaced_by_live_turn_usage() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(100));
    coordinator.hydrate_transcript_from_messages(
        vec![{
            let mut s = UiMessageSnapshot::new("assistant", "history");
            s.usage = Some(UiMessageUsageSnapshot {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(7),
            });
            s
        }],
        None,
    );

    let hydrated = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let hydrated_text: String = hydrated.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(hydrated_text.ends_with("7 (7%)"));

    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 20,
        tool_calls: 0,
        input_tokens: 8,
        output_tokens: 32,
        total_tokens: 40,
    });
    coordinator.drain_transport();

    let live = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let live_text: String = live.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(live_text.ends_with("40 (40%)"));
}

#[test]
fn lane_2_threshold_formatting_contract_100_and_1000_and_11657() {
    let mut state = AppState::default();
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(200_000));

    state.status.tokens.latest_total_tokens = Some(100);
    let one_hundred = crate::runtime::lane_2_status_line_for_test(&state, 40);
    let one_hundred_text: String = one_hundred
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(one_hundred_text.ends_with("100 (0%)"));

    state.status.tokens.latest_total_tokens = Some(1_000);
    let one_thousand = crate::runtime::lane_2_status_line_for_test(&state, 40);
    let one_thousand_text: String = one_thousand
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(one_thousand_text.ends_with("1k (0%)"));

    state.status.tokens.latest_total_tokens = Some(11_657);
    let eleven_point_six = crate::runtime::lane_2_status_line_for_test(&state, 40);
    let eleven_text: String = eleven_point_six
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(eleven_text.ends_with("11.6k (5%)"));
}

#[test]
fn lane_2_is_right_aligned_in_wide_layout() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(11_657),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(200_000));

    let width = 40usize;
    let line = crate::runtime::lane_2_status_line_for_test(&state, width);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(text.chars().count(), width);
    assert!(text.ends_with("11.6k (5%)"));
    assert!(text.starts_with(" "));
}

#[test]
fn lane_2_narrow_width_uses_deterministic_right_anchored_truncation() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(11_657),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(200_000));

    let line = crate::runtime::lane_2_status_line_for_test(&state, 8);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(text, "... (5%)");
}

#[test]
fn lane_2_shows_agent_when_active() {
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

    let line = crate::runtime::lane_2_status_line_for_test(&state, 60);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(text.contains("coder")); // name is present
    assert!(!text.contains("agent:")); // old prefix is gone
    assert!(text.ends_with("42.3k (33%)"));
}

#[test]
fn lane_2_shows_only_tokens_when_no_agent() {
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

    let line = crate::runtime::lane_2_status_line_for_test(&state, 40);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(text.ends_with("250 (25%)"));
    assert!(!text.contains("agent"));
}

#[test]
fn lane_1_no_longer_shows_agent() {
    let mut state = AppState::default();
    state.set_active_agent_identity("coder");

    let lane_1 = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let text: String = lane_1.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(text.starts_with("○ openai/gpt-4o-mini"));
    assert!(!text.contains("coder"));
    assert!(!text.contains("agent"));
    assert!(!text.contains('|'));
}

#[test]
fn hydrate_transcript_sets_latest_total_tokens() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), Some(14000));

    assert_eq!(
        coordinator.state().status.tokens.latest_total_tokens,
        Some(14000)
    );
}

#[test]
fn hydrate_transcript_leaves_latest_total_tokens_none_when_no_value() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), None);

    assert_eq!(coordinator.state().status.tokens.latest_total_tokens, None);
}

#[test]
fn hydrate_assistant_message_with_bold_emits_md_bold_span() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "hello **bold**")],
        None,
    );
    let has_bold = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .filter_map(|b| match &b.source {
            BlockSource::Markdown {
                role: MessageRole::Assistant,
                markdown,
            } => Some(markdown.as_str()),
            _ => None,
        })
        .flat_map(|md| crate::markdown::render_markdown_lines(md, None))
        .flat_map(|l| l.spans.into_iter())
        .any(|s| {
            s.text == "bold" && matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdBold)
        });
    assert!(has_bold);
}

#[test]
fn hydrate_compaction_message_with_italic_emits_md_italic_span() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("compaction", "summary *italic*")],
        None,
    );
    let has_italic = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .filter_map(|b| match &b.source {
            BlockSource::Markdown {
                role: MessageRole::Assistant,
                markdown,
            } => Some(markdown.as_str()),
            _ => None,
        })
        .flat_map(|md| crate::markdown::render_markdown_lines(md, None))
        .flat_map(|l| l.spans.into_iter())
        .any(|s| {
            s.text == "italic"
                && matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdItalic)
        });
    assert!(has_italic);
}

#[test]
fn compact_status_line_reports_lane_1_only() {
    let status_line = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let text: String = status_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    assert!(text.starts_with("○ openai/gpt-4o-mini"));
    assert!(!text.contains('|'));
}
