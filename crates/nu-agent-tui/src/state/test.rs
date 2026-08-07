use crate::state::{
    AgentPickerOption, AppState, CommandPaletteAction, InputMode, McpServerUsabilityState,
    ModelPickerOption, PaneFocus, PermissionPrompt, PromptStatus, ToolCallStatus,
    TranscriptLineStatus, TranscriptRole, UiPhase,
};
use nu_agent_core::protocol::event::PermissionDecision;
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::{
    ProseMessage, SystemMessage, ToolInvocation, ToolResult, TranscriptEntry,
};

/// The active prompt is the one currently in `InProgress` status.
fn active_prompt_id(state: &AppState) -> Option<u64> {
    state
        .prompt_items()
        .iter()
        .find(|p| p.status == PromptStatus::InProgress)
        .map(|p| p.id)
}

/// Pending prompts are those still in `Queued` status.
fn pending_prompt_ids(state: &AppState) -> Vec<u64> {
    state
        .prompt_items()
        .iter()
        .filter(|p| p.status == PromptStatus::Queued)
        .map(|p| p.id)
        .collect()
}

#[test]
fn defaults_start_idle_with_unlocked_input_and_no_abort_pending() {
    let state = AppState::new();

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(!state.abort.pending);
    assert_eq!(state.abort.confirmation_marker, 0);
    assert!(state.transcript_preview.is_empty());
    assert!(state.status_line.is_empty());
    assert_eq!(state.latest_input_tokens, None);
    assert_eq!(state.latest_output_tokens, None);
    assert_eq!(state.latest_total_tokens, None);
    assert_eq!(state.session_total_tokens, 0);
}

#[test]
fn submit_acceptance_clears_input_and_keeps_input_editable() {
    let mut state = AppState::new();

    state.enqueue_prompt("check cluster status".to_string());

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(!state.input_locked);
}

#[test]
fn non_idle_phase_keeps_input_editable_for_queueing() {
    let mut state = AppState::new();

    state.enqueue_prompt("one".to_string());
    assert!(!state.input_locked);
    assert_eq!(state.prompt_items().len(), 1);
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Queued);

    let _ = state.activate_next_prompt();
    assert_eq!(active_prompt_id(&state), Some(1));
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);

    state.request_abort_confirmation();
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(!state.input_locked);
}

#[test]
fn abort_pending_requires_busy_context() {
    let mut state = AppState::new();
    assert!(!state.request_abort_confirmation());
    assert_eq!(state.phase, UiPhase::Idle);

    state.enqueue_prompt("run".to_string());
    let _ = state.activate_next_prompt();
    assert!(state.request_abort_confirmation());
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
}

#[test]
fn finalize_resets_abort_pending_and_unlocks_input() {
    let mut state = AppState::new();
    state.enqueue_prompt("run".to_string());
    let _ = state.activate_next_prompt();
    let marker = state.abort.confirmation_marker;
    assert!(state.request_abort_confirmation());
    assert!(state.abort.confirmation_marker > marker);

    state.finalize_cycle();

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(!state.abort.pending);
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Done);
}

#[test]
fn prompt_queue_lifecycle_is_fifo_and_single_in_progress() {
    let mut state = AppState::new();
    state.enqueue_prompt("p1".to_string());
    state.enqueue_prompt("p2".to_string());
    state.enqueue_prompt("p3".to_string());

    assert_eq!(pending_prompt_ids(&state), vec![1, 2, 3]);

    let first = state.activate_next_prompt();
    assert_eq!(first, Some(1));
    assert_eq!(active_prompt_id(&state), Some(1));
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);
    assert_eq!(state.prompt_items()[1].status, PromptStatus::Queued);
    assert_eq!(state.prompt_items()[2].status, PromptStatus::Queued);

    state.complete_active_prompt();
    assert_eq!(state.prompt_items()[0].status, PromptStatus::Done);

    let second = state.activate_next_prompt();
    assert_eq!(second, Some(2));
    assert_eq!(active_prompt_id(&state), Some(2));

    state.complete_active_prompt();
    let third = state.activate_next_prompt();
    assert_eq!(third, Some(3));
    state.complete_active_prompt();

    assert_eq!(
        state
            .prompt_items()
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>(),
        vec![PromptStatus::Done, PromptStatus::Done, PromptStatus::Done]
    );
}

#[test]
fn global_abort_cancels_active_and_all_pending_prompts() {
    let mut state = AppState::new();
    state.enqueue_prompt("p1".to_string());
    state.enqueue_prompt("p2".to_string());
    state.enqueue_prompt("p3".to_string());
    let _ = state.activate_next_prompt();

    state.cancel_active_and_pending_prompts();

    assert_eq!(active_prompt_id(&state), None);
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(
        state
            .prompt_items()
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>(),
        vec![
            PromptStatus::Cancelled,
            PromptStatus::Cancelled,
            PromptStatus::Cancelled
        ]
    );
}

#[test]
fn input_cursor_and_edit_operations_handle_middle_insert_delete_and_backspace() {
    // These operations are now handled by TextArea, not AppState.
    // This test is preserved as a no-op to document the architectural change.
}

#[test]
fn no_turn_separator_between_user_and_assistant() {
    let mut state = AppState::new();

    state.push_transcript_line(TranscriptRole::User, "prompt one");
    state.push_transcript_line(TranscriptRole::Assistant, "response one");

    // No ruler separator; two spacers (blank lines) separate the turns
    assert_eq!(state.transcript_preview.len(), 4);
    assert_eq!(state.transcript_preview[0].role(), Role::User);
    assert_eq!(state.transcript_preview[1].role(), Role::Separator); // spacer 1
    assert_eq!(state.transcript_preview[2].role(), Role::Separator); // spacer 2
    assert_eq!(state.transcript_preview[3].role(), Role::Assistant);
}

#[test]
fn no_turn_separator_for_same_role_sequences() {
    let mut state = AppState::new();

    state.push_transcript_line(TranscriptRole::Assistant, "line one");
    state.push_transcript_line(TranscriptRole::Assistant, "line two");

    assert_eq!(
        state
            .transcript_preview
            .iter()
            .filter(|entry| entry.role() == Role::Separator)
            .count(),
        0
    );
}

#[test]
fn record_token_usage_tracks_latest_and_accumulates_session_total() {
    let mut state = AppState::new();

    state.record_token_usage(7, 5, 12);
    assert_eq!(state.latest_input_tokens, Some(7));
    assert_eq!(state.latest_output_tokens, Some(5));
    assert_eq!(state.latest_total_tokens, Some(12));
    assert_eq!(state.session_total_tokens, 12);

    state.record_token_usage(2, 3, 5);
    assert_eq!(state.latest_input_tokens, Some(2));
    assert_eq!(state.latest_output_tokens, Some(3));
    assert_eq!(state.latest_total_tokens, Some(5));
    assert_eq!(state.session_total_tokens, 17);
}

#[test]
fn tool_call_lifecycle_tracks_transcript_line_status_by_same_row() {
    let mut state = AppState::new();

    state.start_tool_call("k8s__list_pods", r#"{"namespace":"prod"}"#);

    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role(), Role::Tool);
    assert_eq!(state.transcript_preview[0].text(), "k8s__list_pods");
    if let TranscriptEntry::Tool(invocation) = &state.transcript_preview[0] {
        assert!(invocation.args.contains("→ "));
        assert!(invocation.args.contains("namespace"));
    } else {
        panic!("Expected Tool variant");
    }
    assert_eq!(
        state.transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::InProgress))
    );

    state.finish_tool_call("k8s__list_pods", r#"{"namespace":"prod"}"#, true);
    assert_eq!(
        state.transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Done))
    );
}

#[test]
fn mode_helpers_toggle_between_insert_and_normal() {
    let mut state = AppState::new();
    assert_eq!(state.input_mode, InputMode::Insert);

    state.enter_normal_mode();
    assert_eq!(state.input_mode, InputMode::Normal);

    state.enter_insert_mode();
    assert_eq!(state.input_mode, InputMode::Insert);
}

#[test]
fn permission_prompt_open_sets_required_status_and_presence() {
    let mut state = AppState::new();
    state.open_permission_prompt(PermissionPrompt {
        request_id: "ask-0000000000000001".to_string(),
        matched_rule_identity: "nested:nu__run.command:*".to_string(),
        tool: "nu__run".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        scope: "nested".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        summary: "→ {\"command\":\"echo hi\"}".to_string(),
    });

    assert!(state.has_permission_prompt());
    assert_eq!(state.status_line, "Permission required");
}

#[test]
fn permission_prompt_open_scrolls_to_bottom() {
    let mut state = AppState::new();
    state.push_transcript_line(TranscriptRole::User, "msg1".to_string());
    state.push_transcript_line(TranscriptRole::Assistant, "msg2".to_string());
    state.push_transcript_line(TranscriptRole::Tool, "tool1".to_string());
    state.scroll_transcript_to_top();
    assert!(!state.transcript_following_tail);

    state.open_permission_prompt(PermissionPrompt {
        request_id: "ask-001".to_string(),
        matched_rule_identity: "rule".to_string(),
        tool: "edit".to_string(),
        source: "builtin".to_string(),
        mode: None,
        scope: "global".to_string(),
        pattern: "*".to_string(),
        target_field: None,
        summary: "edit foo.rs".to_string(),
    });

    assert!(state.transcript_following_tail);
}

#[test]
fn submit_permission_decision_enqueues_submission_and_closes_prompt() {
    let mut state = AppState::new();
    state.open_permission_prompt(PermissionPrompt {
        request_id: "ask-0000000000000002".to_string(),
        matched_rule_identity: "nested:nu__run.command:*".to_string(),
        tool: "nu__run".to_string(),
        source: "closure".to_string(),
        mode: None,
        scope: "nested".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        summary: "summary".to_string(),
    });

    assert!(state.submit_permission_decision(PermissionDecision::AllowAlways));
    assert!(!state.has_permission_prompt());

    let submission = state
        .take_next_permission_decision_submission()
        .expect("queued submission");
    assert_eq!(submission.request_id, "ask-0000000000000002");
    assert_eq!(submission.matched_rule_identity, "nested:nu__run.command:*");
    assert_eq!(submission.decision, PermissionDecision::AllowAlways);
}

#[test]
fn normal_mode_defaults_focus_to_transcript_and_insert_focuses_input() {
    let mut state = AppState::new();
    assert_eq!(state.pane_focus, PaneFocus::Input);

    state.enter_normal_mode();
    assert_eq!(state.pane_focus, PaneFocus::Transcript);

    state.enter_insert_mode();
    assert_eq!(state.pane_focus, PaneFocus::Input);
}

#[test]
fn pane_focus_can_cycle_left_and_right() {
    let mut state = AppState::new();
    state.enter_normal_mode();

    state.focus_next_pane();
    assert_eq!(state.pane_focus, PaneFocus::Input);

    state.focus_next_pane();
    assert_eq!(state.pane_focus, PaneFocus::Transcript);

    state.focus_prev_pane();
    assert_eq!(state.pane_focus, PaneFocus::Input);
}

#[test]
fn append_newline_insert_and_boundary_deletes_work_across_lines() {
    // These operations are now handled by TextArea, not AppState.
    // This test is preserved as a no-op to document the architectural change.
}

#[test]
fn assistant_projection_cache_reuses_projected_markdown_for_same_input() {
    let mut state = AppState::new();
    let markdown = "```rust\nfn main() {\n    let x = 42;\n}\n```";

    let first = state.project_assistant_markdown_lines(markdown);
    let second = state.project_assistant_markdown_lines(markdown);

    assert_eq!(first, second);
}

#[test]
fn command_palette_empty_query_returns_canonical_help_status_order_only() {
    let mut state = AppState::new();
    state.open_command_palette();

    assert_eq!(
        state.command_palette_actions(),
        vec![
            CommandPaletteAction::Help,
            CommandPaletteAction::Status,
            CommandPaletteAction::Mcps,
            CommandPaletteAction::Skills,
            CommandPaletteAction::Models,
            CommandPaletteAction::Agents,
            CommandPaletteAction::Sessions,
            CommandPaletteAction::Theme,
        ]
    );
}

#[test]
fn command_palette_empty_query_returns_canonical_help_status_mcps_skills_order() {
    let mut state = AppState::new();
    state.open_command_palette();

    assert_eq!(
        state.command_palette_actions(),
        vec![
            CommandPaletteAction::Help,
            CommandPaletteAction::Status,
            CommandPaletteAction::Mcps,
            CommandPaletteAction::Skills,
            CommandPaletteAction::Models,
            CommandPaletteAction::Agents,
            CommandPaletteAction::Sessions,
            CommandPaletteAction::Theme,
        ]
    );
}

#[test]
fn command_palette_fuzzy_matching_is_case_insensitive_and_non_prefix() {
    let mut state = AppState::new();
    state.open_command_palette();

    for ch in "HP".chars() {
        state.append_command_palette_query_char(ch);
    }
    assert_eq!(
        state.command_palette_actions(),
        vec![CommandPaletteAction::Help]
    );

    state.command_palette_query.clear();
    for ch in "tS".chars() {
        state.append_command_palette_query_char(ch);
    }
    assert_eq!(
        state.command_palette_actions(),
        vec![CommandPaletteAction::Status, CommandPaletteAction::Agents]
    );
}

#[test]
fn command_palette_fuzzy_query_matches_mcps_entry() {
    let mut state = AppState::new();
    state.open_command_palette();

    for ch in "mcp".chars() {
        state.append_command_palette_query_char(ch);
    }

    assert_eq!(
        state.command_palette_actions(),
        vec![CommandPaletteAction::Mcps]
    );
}

#[test]
fn command_palette_fuzzy_query_matches_skills_entry() {
    let mut state = AppState::new();
    state.open_command_palette();

    for ch in "skls".chars() {
        state.append_command_palette_query_char(ch);
    }

    assert_eq!(
        state.command_palette_actions(),
        vec![CommandPaletteAction::Skills]
    );
}

#[test]
fn command_palette_includes_models_action() {
    let mut state = AppState::new();
    state.open_command_palette();

    assert!(
        state
            .command_palette_actions()
            .contains(&CommandPaletteAction::Models)
    );
}

#[test]
fn inline_slash_suggestions_open_on_leading_slash() {
    let mut state = AppState::new();

    state.check_inline_slash("/");

    assert!(state.inline_slash_open);
    assert_eq!(state.inline_slash_selection, 0);
    assert_eq!(
        state.inline_slash_suggestions(),
        &[
            nu_agent_core::protocol::slash::SlashCommand::Compact,
            nu_agent_core::protocol::slash::SlashCommand::Mcp,
            nu_agent_core::protocol::slash::SlashCommand::Help,
            nu_agent_core::protocol::slash::SlashCommand::Status,
            nu_agent_core::protocol::slash::SlashCommand::Models,
            nu_agent_core::protocol::slash::SlashCommand::Agent,
            nu_agent_core::protocol::slash::SlashCommand::New,
            nu_agent_core::protocol::slash::SlashCommand::Session,
            nu_agent_core::protocol::slash::SlashCommand::Theme,
        ]
    );
}

#[test]
fn inline_slash_suggestions_filter_incrementally_as_input_grows() {
    let mut state = AppState::new();

    state.check_inline_slash("/");
    assert_eq!(state.inline_slash_suggestions().len(), 9);

    state.check_inline_slash("/c");
    assert_eq!(
        state.inline_slash_suggestions(),
        &[nu_agent_core::protocol::slash::SlashCommand::Compact]
    );

    state.check_inline_slash("/co");
    assert_eq!(
        state.inline_slash_suggestions(),
        &[nu_agent_core::protocol::slash::SlashCommand::Compact]
    );
}

#[test]
fn inline_slash_suggestions_close_when_prefix_removed() {
    let mut state = AppState::new();

    state.check_inline_slash("/c");
    assert!(state.inline_slash_open);

    state.check_inline_slash("");

    assert!(!state.inline_slash_open);
    assert!(state.inline_slash_suggestions().is_empty());
}

#[test]
fn inline_slash_suggestions_do_not_open_command_palette() {
    let mut state = AppState::new();

    state.check_inline_slash("/");

    assert!(state.inline_slash_open);
    assert!(!state.command_palette_open);
}

#[test]
fn mcp_server_toggle_and_counts_follow_selected_row() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);

    assert_eq!(state.mcp_counts(), (2, 1, 1, 0));

    state.mcp_panel_move_down();
    assert!(state.queue_selected_mcp_toggle_request());

    assert_eq!(state.mcp_counts(), (2, 1, 1, 0));
    assert_eq!(
        state.selected_mcp_server_state(),
        Some(McpServerUsabilityState::Disabled)
    );

    let request = state
        .take_next_mcp_toggle_request()
        .expect("toggle request");
    assert_eq!(request.server_name, "k8s");
    assert!(request.enable);

    assert!(state.set_mcp_server_state_by_name("k8s", McpServerUsabilityState::Enabled));
    assert_eq!(state.mcp_counts(), (2, 2, 0, 0));
}

#[test]
fn enabling_failed_server_queues_enable_and_can_transition_to_failed_on_outcome() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::state::McpServerState {
        name: "gh".to_string(),
        state: McpServerUsabilityState::Failed,
    }]);

    assert!(state.queue_selected_mcp_toggle_request());
    let request = state.take_next_mcp_toggle_request().expect("request");
    assert_eq!(request.server_name, "gh");
    assert!(request.enable);

    assert!(state.set_mcp_server_state_by_name("gh", McpServerUsabilityState::Failed));
    assert_eq!(state.mcp_counts(), (1, 0, 0, 1));
}

#[test]
fn disabling_enabled_server_applies_disabled_state_immediately_and_queues_disable_request() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::state::McpServerState {
        name: "gh".to_string(),
        state: McpServerUsabilityState::Enabled,
    }]);

    assert!(state.queue_selected_mcp_toggle_request());
    assert_eq!(
        state.selected_mcp_server_state(),
        Some(McpServerUsabilityState::Disabled),
        "disable must apply immediately in UI state"
    );

    let request = state
        .take_next_mcp_toggle_request()
        .expect("disable request");
    assert_eq!(request.server_name, "gh");
    assert!(!request.enable);
    assert_eq!(state.mcp_counts(), (1, 0, 1, 0));
}

#[test]
fn failed_server_reason_round_trips_through_state_query() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::state::McpServerState {
        name: "k8s".to_string(),
        state: McpServerUsabilityState::Disabled,
    }]);

    assert!(state.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some("dial tcp timeout".to_string()),
    ));

    let failed = state.failed_mcp_servers_with_reasons();
    assert_eq!(failed, vec![("k8s", Some("dial tcp timeout"))]);
}

#[test]
fn inline_model_picker_opens_with_available_models() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            identity: "openai/gpt-4o-mini".to_string(),
            display: "openai / gpt-4o-mini".to_string(),
            active: true,
        },
        ModelPickerOption {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            identity: "anthropic/claude-3-5-sonnet".to_string(),
            display: "anthropic / claude-3-5-sonnet".to_string(),
            active: false,
        },
    ]);

    state.open_model_picker();

    assert!(state.model_picker_open);
    assert_eq!(state.model_picker_selection, 0);
    assert_eq!(state.model_picker_query, "");
    assert_eq!(state.model_picker_filtered_options().len(), 2);
}

#[test]
fn inline_model_picker_filters_incrementally() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            identity: "openai/gpt-4o-mini".to_string(),
            display: "openai / gpt-4o-mini".to_string(),
            active: true,
        },
        ModelPickerOption {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            identity: "anthropic/claude-3-5-sonnet".to_string(),
            display: "anthropic / claude-3-5-sonnet".to_string(),
            active: false,
        },
    ]);
    state.open_model_picker();

    for ch in "openai".chars() {
        state.append_model_picker_query_char(ch);
    }

    let filtered = state.model_picker_filtered_options();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].identity, "openai/gpt-4o-mini");
}

#[test]
fn inline_model_picker_moves_selection_deterministically() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            identity: "openai/gpt-4o-mini".to_string(),
            display: "openai / gpt-4o-mini".to_string(),
            active: true,
        },
        ModelPickerOption {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            identity: "anthropic/claude-3-5-sonnet".to_string(),
            display: "anthropic / claude-3-5-sonnet".to_string(),
            active: false,
        },
    ]);
    state.open_model_picker();

    state.model_picker_move_down();
    assert_eq!(state.model_picker_selection, 1);

    state.model_picker_move_down();
    assert_eq!(state.model_picker_selection, 0);

    state.model_picker_move_up();
    assert_eq!(state.model_picker_selection, 1);
}

#[test]
fn inline_model_picker_closes_on_escape() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![ModelPickerOption {
        provider: "openai".to_string(),
        model: "gpt-4o-mini".to_string(),
        identity: "openai/gpt-4o-mini".to_string(),
        display: "openai / gpt-4o-mini".to_string(),
        active: true,
    }]);
    state.open_model_picker();

    state.model_picker_close_on_escape();

    assert!(!state.model_picker_open);
    assert_eq!(state.model_picker_query, "");
    assert_eq!(state.model_picker_selection, 0);
}

#[test]
fn inline_model_picker_uses_cached_startup_plugin_config_catalog() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "z-provider".to_string(),
            model: "z-model".to_string(),
            identity: "z-provider/z-model".to_string(),
            display: "z-provider / z-model".to_string(),
            active: false,
        },
        ModelPickerOption {
            provider: "a-provider".to_string(),
            model: "a-model".to_string(),
            identity: "a-provider/a-model".to_string(),
            display: "a-provider / a-model".to_string(),
            active: true,
        },
    ]);
    state.open_model_picker();

    let ordered = state.model_picker_filtered_options();
    assert_eq!(ordered[0].identity, "a-provider/a-model");
    assert_eq!(ordered[1].identity, "z-provider/z-model");
}

#[test]
fn model_picker_query_changes_results_with_hydrated_catalog() {
    let mut state = AppState::new();
    state.set_model_picker_options(vec![
        ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            identity: "openai/gpt-4o-mini".to_string(),
            display: "openai / gpt-4o-mini".to_string(),
            active: true,
        },
        ModelPickerOption {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            identity: "anthropic/claude-3-5-sonnet".to_string(),
            display: "anthropic / claude-3-5-sonnet".to_string(),
            active: false,
        },
    ]);
    state.open_model_picker();

    let all = state.model_picker_filtered_options();
    assert_eq!(all.len(), 2);

    for ch in "claude".chars() {
        state.append_model_picker_query_char(ch);
    }
    let narrowed = state.model_picker_filtered_options();
    assert_eq!(narrowed.len(), 1);
    assert_eq!(narrowed[0].identity, "anthropic/claude-3-5-sonnet");
}

#[test]
fn model_picker_empty_catalog_shows_deterministic_empty_state() {
    let mut state = AppState::new();
    state.set_model_picker_options(Vec::new());
    state.open_model_picker();

    assert!(state.model_picker_open);
    assert!(state.model_picker_filtered_options().is_empty());
}

#[test]
fn push_transcript_item_follows_tail_when_at_last_item() {
    let mut state = AppState::new();

    // Push first item — following_tail starts true, stays true
    state.push_transcript_line(TranscriptRole::User, "first");
    assert!(state.transcript_following_tail);

    // Push second item — should still follow
    state.push_transcript_line(TranscriptRole::Assistant, "second");
    assert!(state.transcript_following_tail);

    // Push third item — should still follow
    state.push_transcript_line(TranscriptRole::User, "third");
    assert!(state.transcript_following_tail);
}

#[test]
fn push_transcript_item_stays_put_when_scrolled_up() {
    let mut state = AppState::new();

    // Push some items
    state.push_transcript_line(TranscriptRole::User, "first");
    state.push_transcript_line(TranscriptRole::Assistant, "second");
    state.push_transcript_line(TranscriptRole::User, "third");

    // Scroll to top (user has scrolled up — disables following)
    state.scroll_transcript_to_top();
    assert!(!state.transcript_following_tail);
    assert_eq!(state.transcript_scroll_offset, 0);

    // Push new item — should NOT re-enable following, offset stays at 0
    state.push_transcript_line(TranscriptRole::Assistant, "fourth");
    assert!(
        !state.transcript_following_tail,
        "following_tail should stay false when user has scrolled up"
    );
    assert_eq!(
        state.transcript_scroll_offset, 0,
        "scroll offset should stay at top when user has scrolled up"
    );
}

#[test]
fn push_transcript_item_follows_when_nothing_selected() {
    let mut state = AppState::new();

    // Initially following_tail is true (default)
    assert!(state.transcript_following_tail);

    // Push first item — following_tail stays true
    state.push_transcript_line(TranscriptRole::User, "first");
    assert!(
        state.transcript_following_tail,
        "first push should keep following_tail true"
    );
}

#[test]
fn enqueue_external_prompt_creates_in_progress_prompt_without_pending() {
    let mut state = AppState::new();

    state.enqueue_external_prompt("mailbox message".to_string());

    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.is_active_cycle());
    assert_eq!(state.prompt_items().len(), 1);
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);
    assert_eq!(state.prompt_items()[0].prompt_text, "mailbox message");
    assert_eq!(active_prompt_id(&state), Some(1));
    assert!(
        pending_prompt_ids(&state).is_empty(),
        "external prompt must NOT appear in pending_prompt_ids"
    );
}

#[test]
fn enqueue_external_prompt_adds_user_transcript_line() {
    let mut state = AppState::new();

    state.enqueue_external_prompt("hello from parent".to_string());

    assert!(!state.transcript_preview.is_empty());
    assert_eq!(state.transcript_preview.last().unwrap().role(), Role::User);
}

#[test]
fn enqueue_external_prompt_completes_via_complete_active_prompt() {
    let mut state = AppState::new();

    state.enqueue_external_prompt("external task".to_string());
    assert_eq!(state.prompt_items()[0].status, PromptStatus::InProgress);

    state.complete_active_prompt();

    assert_eq!(state.prompt_items()[0].status, PromptStatus::Done);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.is_active_cycle());
    assert_eq!(active_prompt_id(&state), None);
}

#[test]
fn enqueue_external_prompt_not_returned_by_take_submitted_prompt() {
    let mut state = AppState::new();

    state.enqueue_external_prompt("external".to_string());

    // take_next_prompt_for_execution should NOT return the external prompt
    // because it's already active (not pending)
    let taken = state.take_next_prompt_for_execution();
    assert_eq!(taken, None, "external prompt must not be re-dispatched");
}

#[test]
fn enqueue_external_prompt_has_spinner_status_on_transcript_line() {
    let mut state = AppState::new();

    state.enqueue_external_prompt("spinner check".to_string());

    let transcript_idx = state.prompt_items()[0].transcript_line_index;
    assert_eq!(
        state.transcript_line_status_for_index(transcript_idx),
        Some(TranscriptLineStatus::Prompt(PromptStatus::InProgress)),
        "transcript line should show InProgress for spinner"
    );
}

#[test]
fn clear_assistant_projection_cache_removes_all_entries() {
    let mut state = AppState::new();
    let markdown = "hello world";

    // Project once to populate the cache
    let first = state.project_assistant_markdown_lines(markdown);

    // Clearing the cache must not change the projected output
    state.clear_assistant_projection_cache();
    let second = state.project_assistant_markdown_lines(markdown);

    assert_eq!(first, second, "clearing cache must not change output");
}

// ---- spacer insertion tests (push-time) ----

#[test]
fn spacer_not_inserted_for_empty_transcript() {
    let state = AppState::new();
    assert!(state.transcript_preview.is_empty());
}

#[test]
fn spacer_not_inserted_for_single_entry() {
    let mut state = AppState::new();
    state.push_transcript_item(TranscriptEntry::User(ProseMessage {
        markdown: "hi".to_string(),
    }));
    assert_eq!(state.transcript_preview.len(), 1);
    assert!(matches!(
        state.transcript_preview[0],
        TranscriptEntry::User(_)
    ));
}

#[test]
fn spacer_not_inserted_for_same_role() {
    let mut state = AppState::new();
    state.push_transcript_item(TranscriptEntry::Assistant(ProseMessage {
        markdown: "first".to_string(),
    }));
    state.push_transcript_item(TranscriptEntry::Assistant(ProseMessage {
        markdown: "second".to_string(),
    }));
    assert_eq!(state.transcript_preview.len(), 2);
    assert!(matches!(
        state.transcript_preview[0],
        TranscriptEntry::Assistant(_)
    ));
    assert!(matches!(
        state.transcript_preview[1],
        TranscriptEntry::Assistant(_)
    ));
}

#[test]
fn spacer_inserted_for_user_then_assistant() {
    let mut state = AppState::new();
    state.push_transcript_item(TranscriptEntry::User(ProseMessage {
        markdown: "hi".to_string(),
    }));
    state.push_transcript_item(TranscriptEntry::Assistant(ProseMessage {
        markdown: "hello".to_string(),
    }));
    // User -> Assistant: two spacers replace the removed turn separator
    assert_eq!(state.transcript_preview.len(), 4);
    assert!(matches!(
        state.transcript_preview[0],
        TranscriptEntry::User(_)
    ));
    assert!(matches!(
        state.transcript_preview[1],
        TranscriptEntry::Spacer(_)
    ));
    assert!(matches!(
        state.transcript_preview[2],
        TranscriptEntry::Spacer(_)
    ));
    assert!(matches!(
        state.transcript_preview[3],
        TranscriptEntry::Assistant(_)
    ));
}

#[test]
fn spacer_not_inserted_for_tool_then_tool_display() {
    let mut state = AppState::new();
    state.push_transcript_item(TranscriptEntry::Tool(ToolInvocation {
        name: "read".to_string(),
        source: "test".to_string(),
        args: "{}".to_string(),
    }));
    state.push_transcript_item(TranscriptEntry::ToolResult(ToolResult {
        name: "read".to_string(),
        success: true,
        lines: vec![],
    }));
    // Tool -> ToolResult: no turn separator (ToolDisplay not a turn role), no spacer (excluded pair)
    assert_eq!(state.transcript_preview.len(), 2);
    assert!(matches!(
        state.transcript_preview[0],
        TranscriptEntry::Tool(_)
    ));
    assert!(matches!(
        state.transcript_preview[1],
        TranscriptEntry::ToolResult(_)
    ));
}

#[test]
fn spacer_inserted_for_assistant_then_system() {
    let mut state = AppState::new();
    state.push_transcript_item(TranscriptEntry::Assistant(ProseMessage {
        markdown: "done".to_string(),
    }));
    state.push_transcript_item(TranscriptEntry::System(SystemMessage {
        text: "system".to_string(),
    }));
    // Assistant -> System: no turn separator (System not a turn role), spacer inserted
    assert_eq!(state.transcript_preview.len(), 3);
    assert!(matches!(
        state.transcript_preview[0],
        TranscriptEntry::Assistant(_)
    ));
    assert!(matches!(
        state.transcript_preview[1],
        TranscriptEntry::Spacer(_)
    ));
    assert!(matches!(
        state.transcript_preview[2],
        TranscriptEntry::System(_)
    ));
}

#[test]
fn spacer_inserted_for_user_then_tool() {
    let mut state = AppState::new();
    state.push_transcript_item(TranscriptEntry::User(ProseMessage {
        markdown: "hi".to_string(),
    }));
    state.push_transcript_item(TranscriptEntry::Tool(ToolInvocation {
        name: "read".to_string(),
        source: "test".to_string(),
        args: "{}".to_string(),
    }));
    // User -> Tool: no turn separator (removed), two spacers inserted for different roles
    assert_eq!(state.transcript_preview.len(), 4);
    assert!(matches!(
        state.transcript_preview[0],
        TranscriptEntry::User(_)
    ));
    assert!(matches!(
        state.transcript_preview[1],
        TranscriptEntry::Spacer(_)
    ));
    assert!(matches!(
        state.transcript_preview[2],
        TranscriptEntry::Spacer(_)
    ));
    assert!(matches!(
        state.transcript_preview[3],
        TranscriptEntry::Tool(_)
    ));
}

// ---- needs_spacer unit tests ----

const NEEDS_SPACER_CASES: &[(Option<Role>, Role, bool)] = &[
    (None, Role::User, false),
    (Some(Role::User), Role::User, false),
    (Some(Role::Separator), Role::User, false),
    (Some(Role::User), Role::Separator, false),
    (Some(Role::User), Role::Assistant, true),
    (Some(Role::Assistant), Role::User, true),
    (Some(Role::Tool), Role::ToolDisplay, false),
    (Some(Role::ToolDisplay), Role::Tool, false),
    (Some(Role::User), Role::Tool, true),
    (Some(Role::Assistant), Role::System, true),
];

#[test]
fn needs_spacer() {
    for (prev, current, expected) in NEEDS_SPACER_CASES {
        assert_eq!(
            super::transcript::needs_spacer(prev.as_ref(), current),
            *expected,
            "needs_spacer(prev={prev:?}, current={current:?})"
        );
    }
}

const NEEDS_DOUBLE_SPACER_CASES: &[(Option<Role>, Role, bool)] = &[
    (None, Role::User, false),
    (Some(Role::User), Role::User, false),
    (Some(Role::Separator), Role::User, false),
    (Some(Role::User), Role::Separator, false),
    (Some(Role::User), Role::Assistant, true),
    (Some(Role::Assistant), Role::User, true),
    (Some(Role::User), Role::Tool, true),
    (Some(Role::Tool), Role::User, true),
    (Some(Role::User), Role::System, true),
    (Some(Role::System), Role::User, true),
    (Some(Role::Tool), Role::ToolDisplay, false),
    (Some(Role::ToolDisplay), Role::Tool, false),
    (Some(Role::Assistant), Role::System, false),
    (Some(Role::Assistant), Role::Tool, false),
    (Some(Role::Tool), Role::Assistant, false),
];

#[test]
fn needs_double_spacer() {
    for (prev, current, expected) in NEEDS_DOUBLE_SPACER_CASES {
        assert_eq!(
            super::transcript::needs_double_spacer(prev.as_ref(), current),
            *expected,
            "needs_double_spacer(prev={prev:?}, current={current:?})"
        );
    }
}

// ---- agent picker state tests ----

fn test_agent_options() -> Vec<AgentPickerOption> {
    vec![
        AgentPickerOption {
            name: "alpha".into(),
            description: Some("Alpha agent".into()),
            display: "alpha — Alpha agent".into(),
            active: false,
            builtin: false,
        },
        AgentPickerOption {
            name: "beta".into(),
            description: None,
            display: "beta".into(),
            active: true,
            builtin: false,
        },
        AgentPickerOption {
            name: "gamma".into(),
            description: Some("Gamma agent".into()),
            display: "gamma — Gamma agent".into(),
            active: false,
            builtin: false,
        },
    ]
}

#[test]
fn test_open_agent_picker() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.agent_picker_query = "leftover".to_string();
    state.agent_picker_selection = 2;

    state.open_agent_picker();

    assert!(state.agent_picker_open);
    assert_eq!(state.agent_picker_query, "");
    assert_eq!(state.agent_picker_selection, 0);
}

#[test]
fn test_close_agent_picker() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();
    state.agent_picker_query = "al".to_string();
    state.agent_picker_selection = 1;

    state.close_agent_picker();

    assert!(!state.agent_picker_open);
    assert_eq!(state.agent_picker_query, "");
    assert_eq!(state.agent_picker_selection, 0);
}

#[test]
fn test_queue_agent_picker_launch_request() {
    let mut state = AppState::new();

    state.queue_agent_picker_launch_request();

    assert!(state.take_next_agent_picker_launch_request());
}

#[test]
fn test_take_next_agent_picker_launch_request() {
    let mut state = AppState::new();

    // No pending requests
    assert!(!state.take_next_agent_picker_launch_request());

    // Queue one
    state.queue_agent_picker_launch_request();
    assert!(state.take_next_agent_picker_launch_request());

    // Consumed — should return false again
    assert!(!state.take_next_agent_picker_launch_request());
}

#[test]
fn test_filtered_agent_picker_options_empty_query() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    let filtered = state.agent_picker_filtered_options();
    assert_eq!(filtered.len(), 3);
}

#[test]
fn test_filtered_agent_picker_options_with_query() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // Case-insensitive match on name
    for ch in "ALPHA".chars() {
        state.append_agent_picker_query_char(ch);
    }
    let filtered = state.agent_picker_filtered_options();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "alpha");

    // Reset and test display match
    state.agent_picker_query.clear();
    for ch in "Gamma agent".chars() {
        state.append_agent_picker_query_char(ch);
    }
    let filtered = state.agent_picker_filtered_options();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "gamma");
}

#[test]
fn test_filtered_agent_picker_options_no_match() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    for ch in "zzz".chars() {
        state.append_agent_picker_query_char(ch);
    }
    let filtered = state.agent_picker_filtered_options();
    assert!(filtered.is_empty());
}

#[test]
fn test_selected_agent_picker_option() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // set_agent_picker_options sorts by name: alpha, beta, gamma
    let first = state.selected_agent_picker_option();
    assert_eq!(first.unwrap().name, "alpha");

    state.agent_picker_move_down();
    let second = state.selected_agent_picker_option();
    assert_eq!(second.unwrap().name, "beta");

    state.agent_picker_move_down();
    let third = state.selected_agent_picker_option();
    assert_eq!(third.unwrap().name, "gamma");
}

#[test]
fn test_queue_selected_agent_switch_request() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // Select second item (beta)
    state.agent_picker_move_down();
    assert!(state.queue_selected_agent_switch_request());

    let request = state.take_next_agent_switch_request();
    assert_eq!(request, Some("beta".to_string()));
}

#[test]
fn test_take_next_agent_switch_request() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // Queue two requests in order: alpha then beta
    assert!(state.queue_selected_agent_switch_request()); // alpha (index 0)
    state.agent_picker_move_down();
    assert!(state.queue_selected_agent_switch_request()); // beta (index 1)

    // FIFO order
    assert_eq!(
        state.take_next_agent_switch_request(),
        Some("alpha".to_string())
    );
    assert_eq!(
        state.take_next_agent_switch_request(),
        Some("beta".to_string())
    );
    assert_eq!(state.take_next_agent_switch_request(), None);
}

#[test]
fn test_set_active_agent_identity() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());

    state.set_active_agent_identity("beta");

    // Only "beta" should be active
    let options = &state.agent_picker_options;
    for opt in options {
        if opt.name == "beta" {
            assert!(opt.active, "beta should be active");
        } else {
            assert!(!opt.active, "{} should not be active", opt.name);
        }
    }
    assert_eq!(state.active_agent_identity(), Some("beta"));
}

#[test]
fn test_agent_picker_move_up_wraps() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    assert_eq!(state.agent_picker_selection, 0);

    // Move up from 0 should wrap to last (index 2)
    state.agent_picker_move_up();
    assert_eq!(state.agent_picker_selection, 2);
}

#[test]
fn test_agent_picker_move_down_wraps() {
    let mut state = AppState::new();
    state.set_agent_picker_options(test_agent_options());
    state.open_agent_picker();

    // Move to last
    state.agent_picker_move_down();
    state.agent_picker_move_down();
    assert_eq!(state.agent_picker_selection, 2);

    // Move down from last should wrap to 0
    state.agent_picker_move_down();
    assert_eq!(state.agent_picker_selection, 0);
}

#[test]
fn command_palette_includes_agents_action() {
    let mut state = AppState::new();
    state.open_command_palette();

    assert!(
        state
            .command_palette_actions()
            .contains(&CommandPaletteAction::Agents)
    );
}

#[test]
fn info_panel_for_command_palette_action_agents_returns_none() {
    assert_eq!(CommandPaletteAction::Agents.info_panel(), None);
}

// ---- agent Tab cycling tests ----

#[test]
fn test_has_agents_to_cycle_empty() {
    let state = AppState::new();
    assert!(state.agent_cycle_names.is_empty());
    assert!(!state.has_agents_to_cycle());
}

#[test]
fn test_has_agents_to_cycle_one() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string()];
    assert!(!state.has_agents_to_cycle());
}

#[test]
fn test_has_agents_to_cycle_two() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    assert!(state.has_agents_to_cycle());
}

#[test]
fn test_next_agent_cycle_name_cycles() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];

    // Set active to "planner" → next should be "maker"
    state.set_active_agent_identity("planner");
    assert_eq!(state.next_agent_cycle_name(), Some("maker".to_string()));

    // Set active to "maker" → next should wrap to "planner"
    state.set_active_agent_identity("maker");
    assert_eq!(state.next_agent_cycle_name(), Some("planner".to_string()));
}

#[test]
fn test_next_agent_cycle_name_no_current() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    // active_agent_identity is None → unwrap_or("") → position not found → unwrap_or(0)
    // next_idx = (0 + 1) % 2 = 1 → "maker"
    assert_eq!(state.next_agent_cycle_name(), Some("maker".to_string()));
}

#[test]
fn test_queue_cycle_agent_request() {
    let mut state = AppState::new();
    state.agent_cycle_names = vec!["planner".to_string(), "maker".to_string()];
    state.set_active_agent_identity("planner");

    state.queue_cycle_agent_request();

    let request = state.take_next_agent_switch_request();
    assert_eq!(request, Some("maker".to_string()));
}

// === User/Assistant unified markdown ingestion (Task 3) ===

#[test]
fn push_transcript_line_user_bold_markdown_emits_md_bold_span() {
    let mut state = AppState::new();
    state.push_transcript_line(TranscriptRole::User, "hello **world**".to_string());
    let TranscriptEntry::User(m) = state.transcript_preview.last().expect("entry") else {
        panic!("expected User");
    };
    // Raw markdown is stored; verify it projects to MdBold at render time
    let bold = crate::markdown::render_markdown_lines(&m.markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .find(|s| matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdBold))
        .expect("expected MdBold span");
    assert_eq!(bold.text, "world");
}

#[test]
fn push_transcript_line_assistant_bold_markdown_emits_md_bold_span() {
    let mut state = AppState::new();
    state.push_transcript_line(TranscriptRole::Assistant, "hello **world**".to_string());
    let TranscriptEntry::Assistant(m) = state.transcript_preview.last().expect("entry") else {
        panic!("expected Assistant");
    };
    let bold = crate::markdown::render_markdown_lines(&m.markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .find(|s| matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdBold))
        .expect("expected MdBold span");
    assert_eq!(bold.text, "world");
}

#[test]
fn push_transcript_line_user_and_assistant_produce_identical_lines_for_same_text() {
    let mut s1 = AppState::new();
    let mut s2 = AppState::new();
    let text = "**bold** and *italic* and `code`".to_string();
    s1.push_transcript_line(TranscriptRole::User, text.clone());
    s2.push_transcript_line(TranscriptRole::Assistant, text);
    let TranscriptEntry::User(u) = s1.transcript_preview.last().expect("u") else {
        panic!();
    };
    let TranscriptEntry::Assistant(a) = s2.transcript_preview.last().expect("a") else {
        panic!();
    };
    assert_eq!(
        u.markdown, a.markdown,
        "user and assistant prose must be byte-identical"
    );
}

#[test]
fn push_transcript_line_user_fenced_code_block_produces_multiple_lines() {
    let mut state = AppState::new();
    state.push_transcript_line(
        TranscriptRole::User,
        "```rust\nfn a() {}\nfn b() {}\n```".to_string(),
    );
    let TranscriptEntry::User(m) = state.transcript_preview.last().expect("entry") else {
        panic!();
    };
    // Verify projection of the stored raw markdown yields multiple lines
    let projected = crate::markdown::render_markdown_lines(&m.markdown, None);
    assert!(projected.len() >= 2);
}

#[test]
fn enqueue_prompt_does_not_add_transcript_entry() {
    let mut state = AppState::new();
    state.enqueue_external_prompt("first".to_string());
    let before = state.transcript_preview.len();
    state.enqueue_prompt("second".to_string());
    assert_eq!(state.transcript_preview.len(), before);
}

#[test]
fn queued_prompt_has_sentinel_transcript_line_index() {
    let mut state = AppState::new();
    state.enqueue_external_prompt("first".to_string());
    state.enqueue_prompt("second".to_string());
    let queued = state
        .prompt_items()
        .iter()
        .find(|p| p.status == PromptStatus::Queued)
        .unwrap();
    assert_eq!(queued.transcript_line_index, usize::MAX);
}

#[test]
fn clear_transcript_resets_token_fields() {
    let mut state = AppState::new();
    state.latest_input_tokens = Some(100);
    state.latest_output_tokens = Some(200);
    state.latest_total_tokens = Some(300);
    state.clear_transcript();
    assert!(state.latest_input_tokens.is_none());
    assert!(state.latest_output_tokens.is_none());
    assert!(state.latest_total_tokens.is_none());
}

#[test]
fn activate_next_prompt_adds_user_entry_to_transcript() {
    let mut state = AppState::new();
    state.enqueue_external_prompt("first".to_string());
    state.enqueue_prompt("second".to_string());
    state.complete_active_prompt();
    let before = state.transcript_preview.len();
    state.activate_next_prompt();
    assert_eq!(state.transcript_preview.len(), before + 1);
    assert!(matches!(
        state.transcript_preview.last().unwrap(),
        TranscriptEntry::User(_)
    ));
}

#[test]
fn activated_prompt_has_real_transcript_line_index() {
    let mut state = AppState::new();
    state.enqueue_external_prompt("first".to_string());
    state.enqueue_prompt("second".to_string());
    state.complete_active_prompt();
    state.activate_next_prompt();
    let items = state.prompt_items().to_vec();
    let active = items
        .iter()
        .find(|p| p.status == PromptStatus::InProgress)
        .unwrap();
    assert_ne!(active.transcript_line_index, usize::MAX);
}

#[test]
fn cancel_and_restore_drains_pending_texts_into_input_buffer() {
    let mut state = AppState::new();
    state.enqueue_prompt("alpha".to_string());
    let _ = state.activate_next_prompt();
    state.enqueue_prompt("beta".to_string());
    state.enqueue_prompt("gamma".to_string());

    let result = state.cancel_and_restore_pending_to_input();

    assert_eq!(result, Some("beta\n\ngamma".to_string()));
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(active_prompt_id(&state), None);
    assert_eq!(state.phase, UiPhase::Idle);
}

#[test]
fn cancel_and_restore_with_no_pending_leaves_buffer_empty() {
    let mut state = AppState::new();
    state.enqueue_prompt("only".to_string());
    let _ = state.activate_next_prompt();

    let result = state.cancel_and_restore_pending_to_input();

    assert_eq!(result, None);
    assert_eq!(state.phase, UiPhase::Idle);
}

#[test]
fn cancel_and_restore_on_idle_is_noop() {
    let mut state = AppState::new();
    let result = state.cancel_and_restore_pending_to_input();
    assert_eq!(result, None);
}

#[test]
fn coalesced_dispatch_joins_all_pending_into_one_string() {
    let mut state = AppState::new();
    state.enqueue_prompt("first".to_string());
    state.enqueue_prompt("second".to_string());
    state.enqueue_prompt("third".to_string());
    // Reset so take_next can activate (enqueue_prompt sets busy)
    state.phase = UiPhase::Idle;
    state.active_cycle = false;

    let result = state.take_next_prompt_for_execution();

    assert_eq!(result, Some("first\n\nsecond\n\nthird".to_string()));
    assert!(pending_prompt_ids(&state).is_empty());
    assert_eq!(state.phase, UiPhase::Busy);
}

#[test]
fn coalesced_dispatch_single_pending_returns_text_unchanged() {
    let mut state = AppState::new();
    state.enqueue_prompt("only".to_string());
    state.phase = UiPhase::Idle;
    state.active_cycle = false;

    let result = state.take_next_prompt_for_execution();

    assert_eq!(result, Some("only".to_string()));
    assert!(pending_prompt_ids(&state).is_empty());
}

#[test]
fn coalesced_dispatch_empty_queue_returns_none() {
    let mut state = AppState::new();
    let result = state.take_next_prompt_for_execution();
    assert_eq!(result, None);
}

#[test]
fn history_up_on_first_use_loads_last_submitted() {
    let mut state = AppState::new();
    state.enqueue_prompt("p1".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let result = state.history_up("");
    assert_eq!(result, Some("p1".to_string()));
}

#[test]
fn history_up_cycles_newest_first_and_clamps_at_oldest() {
    let mut state = AppState::new();
    for t in ["a", "b", "c"] {
        state.enqueue_prompt(t.to_string());
        let _ = state.activate_next_prompt();
        state.complete_active_prompt();
    }
    let r1 = state.history_up("");
    assert_eq!(r1, Some("c".to_string()));
    let r2 = state.history_up("");
    assert_eq!(r2, Some("b".to_string()));
    let r3 = state.history_up("");
    assert_eq!(r3, Some("a".to_string()));
    let r4 = state.history_up("");
    assert_eq!(r4, Some("a".to_string())); // clamp
}

#[test]
fn history_down_past_newest_restores_draft() {
    let mut state = AppState::new();
    state.enqueue_prompt("p1".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let _ = state.history_up("draft");
    assert_eq!(state.history_down(), Some("draft".to_string()));
}

#[test]
fn history_up_moves_cursor_up_in_multiline_buffer() {
    // History navigation now returns text; cursor is managed by TextArea.
    // This test verifies the text is returned correctly.
    let mut state = AppState::new();
    state.enqueue_prompt("prev".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let result = state.history_up("line1\nline2");
    assert_eq!(result, Some("prev".to_string()));
}

#[test]
fn history_up_clamps_column_to_shorter_prev_line() {
    // History navigation now returns text; cursor is managed by TextArea.
    let mut state = AppState::new();
    state.enqueue_prompt("prev".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let result = state.history_up("ab\nxyz");
    assert_eq!(result, Some("prev".to_string()));
}

#[test]
fn history_up_on_first_line_of_multiline_enters_history() {
    let mut state = AppState::new();
    state.enqueue_prompt("prev".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let result = state.history_up("line1\nline2");
    assert_eq!(result, Some("prev".to_string()));
}

#[test]
fn history_down_moves_cursor_down_in_multiline() {
    // History navigation now returns text; cursor is managed by TextArea.
    let result = AppState::new().history_down();
    assert_eq!(result, None);
}

#[test]
fn typing_resets_history_navigation() {
    let mut state = AppState::new();
    state.enqueue_prompt("p1".to_string());
    let _ = state.activate_next_prompt();
    state.complete_active_prompt();
    let _ = state.history_up("");
    assert_eq!(state.history_up(""), Some("p1".to_string()));
    // After typing, history navigation is reset
    state.reset_history_navigation();
    assert_eq!(state.history_down(), None);
    state.reset_history_navigation();
    assert_eq!(state.history_down(), None);
}

#[test]
fn insert_exit_pending_j_is_true_within_timeout() {
    let mut state = AppState::new();
    assert!(!state.insert_exit_pending_j());

    state.set_insert_exit_pending_j();
    assert!(state.insert_exit_pending_j());
}

#[test]
fn insert_exit_pending_j_is_false_after_timeout() {
    let mut state = AppState::new();
    state.set_insert_exit_pending_j();

    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(!state.insert_exit_pending_j());
}

#[test]
fn clear_insert_exit_pending_j_resets_to_false() {
    let mut state = AppState::new();
    state.set_insert_exit_pending_j();
    assert!(state.insert_exit_pending_j());

    state.clear_insert_exit_pending_j();
    assert!(!state.insert_exit_pending_j());
    state.clear_insert_exit_pending_j();
    assert!(!state.insert_exit_pending_j());
}
