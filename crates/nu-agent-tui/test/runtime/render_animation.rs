use super::*;

#[test]
fn render_needed_is_true_after_drain_transport() {
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    // Clear the initial render_needed flag via render_if_needed
    coord.set_render_needed(false);
    assert!(!coord.render_needed());

    coord.enqueue_ui_event(UiEvent::Tick);
    coord.drain_transport();

    assert!(
        coord.render_needed(),
        "render_needed should be true after drain_transport processes events"
    );
}

#[test]
fn render_if_needed_skips_when_not_dirty() {
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.set_render_needed(false);
    coord.set_last_render_at(Instant::now() - Duration::from_secs(1));

    let mut live: Option<
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    > = None;
    let result = coord.render_if_needed(&mut live);

    assert!(result.is_ok());
    assert!(
        !coord.render_needed(),
        "render_needed should remain false when not dirty"
    );
}

#[test]
fn render_if_needed_skips_when_too_soon() {
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.set_render_needed(true);
    coord.set_last_render_at(Instant::now());

    let mut live: Option<
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    > = None;
    let result = coord.render_if_needed(&mut live);

    assert!(result.is_ok());
    assert!(
        coord.render_needed(),
        "render_needed should remain true when frame interval has not elapsed"
    );
}

#[test]
fn render_if_needed_fires_when_dirty_and_elapsed() {
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.set_render_needed(true);
    coord.set_last_render_at(Instant::now() - Duration::from_secs(1));

    let mut live: Option<
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    > = None;
    let result = coord.render_if_needed(&mut live);

    assert!(result.is_ok());
    assert!(
        !coord.render_needed(),
        "render_needed should be false after render_if_needed fires"
    );
}

#[test]
fn has_active_animation_true_when_busy() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.state.phase = UiPhase::Busy;

    // -- Exec & Check
    assert!(coord.has_active_animation());
}

#[test]
fn has_active_animation_true_when_abort_pending() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.state.phase = UiPhase::AbortPending;

    // -- Exec & Check
    assert!(coord.has_active_animation());
}

#[test]
fn has_active_animation_true_when_compaction_in_progress() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    let mut evicted = 0usize;
    coord
        .state
        .compaction
        .start_block(&mut coord.state.transcript, "test", &mut evicted);

    // -- Exec & Check
    assert!(coord.has_active_animation());
}

#[test]
fn has_active_animation_true_when_in_progress_entry() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    push_entry(&mut coord, Some(ItemStatus::InProgress));

    // -- Exec & Check
    assert!(coord.has_active_animation());
}

#[test]
fn has_active_animation_false_when_only_finished_entries() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    push_entry(&mut coord, Some(ItemStatus::Done));
    push_entry(&mut coord, Some(ItemStatus::Failed));
    push_entry(&mut coord, Some(ItemStatus::Queued));

    // -- Exec & Check
    assert!(!coord.has_active_animation());
}

#[test]
fn has_active_animation_false_when_fully_idle() {
    // -- Setup & Fixtures
    let coord = RuntimeCoordinator::new(80, 24, None);

    // -- Exec & Check
    assert!(!coord.has_active_animation());
}

#[test]
fn expire_status_message_if_due_clears_expired_message() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.state.status.message.set_message("stale");

    // -- Exec & Check
    assert!(coord.expire_status_message_if_due(Instant::now() + Duration::from_secs(5)));
    assert!(coord.state.status.message.status_line().is_empty());
}

#[test]
fn expire_status_message_if_due_keeps_fresh_message() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.state.status.message.set_message("fresh");

    // -- Exec & Check
    assert!(!coord.expire_status_message_if_due(Instant::now()));
    assert_eq!(coord.state.status.message.status_line(), "fresh");
}

#[test]
fn expire_status_message_if_due_keeps_warning_within_ttl() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.state.status.message.set_warning("warned");

    // -- Exec & Check
    assert!(!coord.expire_status_message_if_due(Instant::now() + Duration::from_secs(5)));
    assert_eq!(coord.state.status.message.status_line(), "warned");
}

#[test]
fn test_coordinator_has_pending_status_message_false_when_fresh() {
    // -- Setup & Fixtures
    let coord = RuntimeCoordinator::new(80, 24, None);

    // -- Exec & Check
    assert!(!coord.has_pending_status_message());
}

#[test]
fn test_coordinator_has_pending_status_message_true_after_set_message() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);

    // -- Exec
    coord.state.status.message.set_message("info");

    // -- Check
    assert!(coord.has_pending_status_message());
}

#[test]
fn test_coordinator_has_pending_status_message_false_after_expiry_clears() {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.state.status.message.set_message("stale");

    // -- Exec
    let expired = coord.expire_status_message_if_due(Instant::now() + Duration::from_secs(5));

    // -- Check
    assert!(expired);
    assert!(!coord.has_pending_status_message());
}

#[test]
fn drain_transport_coalesces_consecutive_assistant_messages() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    for text in ["a", "ab", "abc", "abcd", "abcde"] {
        coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
            text: text.to_string(),
        });
    }
    coordinator.drain_transport();

    // Only the last message ("abcde") should have been processed through the reducer

    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect();
    assert!(
        texts.contains(&"abcde".to_string()),
        "transcript should contain the final coalesced text: {texts:?}"
    );
    assert!(
        !texts.contains(&"abcd".to_string()),
        "intermediate messages should not appear in transcript: {texts:?}"
    );
}

#[test]
fn drain_transport_preserves_order_with_mixed_events() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "hello".to_string(),
    });
    coordinator.enqueue_ui_event(UiEvent::Tick);
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "world".to_string(),
    });
    coordinator.drain_transport();

    // Both messages should have been processed, because a Tick
    // separates them — coalescing only applies to consecutive same-type events

    // Final transcript shows "world" (the second AssistantMessage replaces the first)
    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(
                b.source,
                BlockSource::Markdown {
                    role: MessageRole::Assistant,
                    ..
                }
            )
        })
        .map(|block| block.source.plain_text())
        .collect();
    assert_eq!(texts, vec!["world"]);
}

#[test]
fn drain_transport_single_assistant_message_not_affected() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "solo".to_string(),
    });
    coordinator.drain_transport();

    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(
                b.source,
                BlockSource::Markdown {
                    role: MessageRole::Assistant,
                    ..
                }
            )
        })
        .map(|block| block.source.plain_text())
        .collect();
    assert_eq!(texts, vec!["solo"]);
}

#[test]
fn ui_event_warning_sets_status_line_and_marks_render_needed() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);

    // -- Exec
    // Mirrors the interactive loop's ui_event arm: reduce, then mark on true.
    let handled = coordinator.reduce_ui_event(UiEvent::Warning {
        message: "warned".to_string(),
    });
    if handled {
        coordinator.mark_render_needed();
    }

    // -- Check
    assert!(handled, "StatusState must claim plain warning messages");
    assert_eq!(coordinator.state.status.message.status_line(), "warned");
    assert!(
        coordinator.render_needed(),
        "handled warnings must mark the frame dirty"
    );
}

#[test]
fn ui_event_turn_error_falls_through_to_transcript_and_finalize() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.state.phase = UiPhase::Busy;
    coordinator.state.input_locked = true;

    // -- Exec
    let handled = coordinator.reduce_ui_event(UiEvent::TurnError {
        message: "boom".to_string(),
    });

    // -- Check
    assert!(handled, "TurnError must fall through and be handled");
    let error_line = coordinator.state.transcript.blocks().iter().any(|b| {
        block_to_role(b) == TranscriptRole::System && b.source.plain_text().contains("Error: boom")
    });
    assert!(error_line, "TurnError must land on the transcript");
    assert_eq!(
        coordinator.state.phase,
        UiPhase::Idle,
        "TurnError finalize must return to Idle"
    );
    assert!(!coordinator.state.input_locked);
}

#[test]
fn reduce_ui_state_event_status_variants_route_through_status_state() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec & Check
    coordinator.reduce_ui_state_event(UiStateEvent::SetActiveModelIdentity(
        "openai/gpt-4o".to_string(),
    ));
    coordinator.mark_render_needed();
    assert_eq!(
        coordinator.state.status.identity.active_model_identity,
        "openai/gpt-4o"
    );
    assert!(coordinator.render_needed());

    coordinator.reduce_ui_state_event(UiStateEvent::SetActivePersonaIcon(Some("icon".to_string())));
    assert_eq!(
        coordinator.state.status.identity.active_persona_icon,
        Some("icon".to_string())
    );

    coordinator.reduce_ui_state_event(UiStateEvent::SetContextWindowMaxTokens(Some(128_000)));
    assert_eq!(
        coordinator.state.status.tokens.context_window_max_tokens,
        Some(128_000)
    );

    coordinator.reduce_ui_state_event(UiStateEvent::SetMcpServerState {
        server: "gh".to_string(),
        state: nu_agent_core::protocol::contracts::McpUsabilityState::Failed,
        error: Some("boom".to_string()),
        total: 3,
    });
    assert_eq!(coordinator.state.status.mcp.llm_visible_mcp_tool_count, 3);

    coordinator.reduce_ui_state_event(UiStateEvent::SetMcpVisibleToolCount {
        server: "gh".to_string(),
        count: 5,
    });
    assert_eq!(
        coordinator
            .state
            .status
            .mcp
            .mcp_visible_tool_count_for_server_name("gh"),
        5
    );

    coordinator.reduce_ui_state_event(UiStateEvent::SetMcpVisibleToolNames {
        server: "gh".to_string(),
        names: vec!["z_tool".to_string(), "a_tool".to_string()],
    });
    assert_eq!(
        coordinator
            .state
            .status
            .mcp
            .mcp_visible_tool_names_for_server_name("gh"),
        vec!["a_tool".to_string(), "z_tool".to_string()]
    );
}

#[test]
fn reduce_ui_state_event_non_status_variants_fall_back_to_app_state() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    assert!(coordinator.state.transcript.blocks().is_empty());

    // -- Exec
    // PushStartupLogo is not status-owned: StatusState returns false and
    // AppState handles it (mirrors the ui_state_rx caller seam).
    coordinator.reduce_ui_state_event(UiStateEvent::PushStartupLogo);

    // -- Check
    let has_logo = coordinator
        .state
        .transcript
        .blocks()
        .iter()
        .any(|b| matches!(b.source, BlockSource::Banner { .. }));
    assert!(
        has_logo,
        "non-status UiStateEvent must fall through to AppState"
    );
}

#[test]
fn ui_event_permission_requested_applies_all_effects() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);
    coordinator.state.status.message.set_message("idle");
    coordinator.state.scroll.following_tail = false;

    let event = UiEvent::PermissionRequested {
        request_id: "ask-0000000000000001".to_string(),
        context: PermissionRequestContext {
            tool: "nu(command=echo hi)".to_string(),
            tool_key: "nu\n{\"command\":\"echo hi\"}".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
            pre_authorize_display: None,
        },
    };

    // -- Exec
    // Mirrors the render loop's ui_event arm: reduce, then mark the frame
    // dirty when the reducer reports a change.
    let handled = coordinator.reduce_ui_event(event);
    if handled {
        coordinator.mark_render_needed();
    }

    // -- Check
    assert!(handled, "PermissionRequested must reduce to true");
    assert_eq!(coordinator.state.status.message.status_line(), "idle");
    assert!(
        coordinator.state.scroll.following_tail,
        "PermissionRequested must scroll the transcript to the bottom"
    );
    assert!(
        coordinator.render_needed(),
        "PermissionRequested must mark the frame dirty"
    );
    assert!(
        coordinator.state.permission.has_prompt(),
        "PermissionRequested must open the permission prompt"
    );
}

#[test]
fn ui_event_permission_decision_variants_skip_effects() {
    for event in [
        UiEvent::PermissionDecisionSubmitted {
            request_id: "ask-0000000000000001".to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "nested:nu.command:*".to_string(),
        },
        UiEvent::PermissionDecisionTimedOut {
            request_id: "ask-0000000000000001".to_string(),
        },
        UiEvent::PermissionDecisionIgnored {
            request_id: "ask-0000000000000001".to_string(),
            reason: "user closed".to_string(),
        },
    ] {
        // -- Setup & Fixtures
        let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
        coordinator.set_render_needed(false);
        coordinator.state.status.message.set_message("idle");
        coordinator.state.scroll.following_tail = false;

        // -- Exec
        let handled = coordinator.reduce_ui_event(event);
        if handled {
            coordinator.mark_render_needed();
        }

        // -- Check
        assert!(!handled, "decision variants must reduce to false");
        assert_eq!(
            coordinator.state.status.message.status_line(),
            "idle",
            "decision variants must leave status_line unchanged"
        );
        assert!(
            !coordinator.state.scroll.following_tail,
            "decision variants must not scroll the transcript"
        );
        assert!(
            !coordinator.render_needed(),
            "decision variants must not mark the frame dirty"
        );
    }
}

#[test]
fn ui_event_permission_pre_authorize_display_applied_before_reduce() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    // The preview attaches to a pending tool call, so start one first — the
    // same sequence the runtime produces for previewable tools.
    coordinator.reduce_ui_event(UiEvent::ToolStarted {
        name: "write".to_string(),
        source: "user".to_string(),
        arguments: "{}".to_string(),
        call_line: nu_agent_core::protocol::tool_args::CallLine::from_json_summary("{}"),
    });
    let context = PermissionRequestContext {
        tool: "write".to_string(),
        tool_key: "write\n{}".to_string(),
        source: "user".to_string(),
        mode: Some("edit".to_string()),
        matched_rule_identity: "identity".to_string(),
        scope: "scope".to_string(),
        target_field: Some("target".to_string()),
        pattern: "pattern".to_string(),
        summary: "summary".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "preview-title".to_string(),
            sections: vec![],
        }),
    };

    // -- Exec
    // The render loop's ui_event arm dispatches through reduce_ui_event, which
    // applies the pre-authorize display before opening the prompt.
    let handled = coordinator.reduce_ui_event(UiEvent::PermissionRequested {
        request_id: "ask-0000000000000001".to_string(),
        context,
    });

    // -- Check
    assert!(handled, "PermissionRequested must reduce to true");
    // The preview is pushed as its own ToolDisplay block directly after the
    // pending Tool block.
    let preview_pushed = coordinator
        .state
        .transcript
        .blocks()
        .iter()
        .any(|b| matches!(b.source, BlockSource::ToolDisplay { .. }));
    assert!(
        preview_pushed,
        "pre_authorize_display must be pushed as a ToolDisplay block before reduce"
    );
    assert!(
        coordinator.state.permission.has_prompt(),
        "reduce must still open the prompt after the display is applied"
    );
}
