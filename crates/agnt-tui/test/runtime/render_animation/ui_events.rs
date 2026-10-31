use super::*;

use agnt_core::transcript::entry::TranscriptEntry;

use agnt_core::protocol::events::{AutoResolutionAction, RequestId, SystemKind};

/// A warning SystemEvent sets the status line and marks the frame dirty; a
/// TurnError falls through to the transcript and finalizes the cycle.
#[test]
fn ui_event_warning_and_turn_error() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);

    // -- Exec
    let handled = coordinator.reduce_system_event(SystemEvent::SystemMessage {
        kind: SystemKind::Warning,
        text: "warned".to_string(),
    });
    if handled {
        coordinator.mark_render_needed();
    }

    // -- Check
    assert!(handled, "StatusState must claim plain warning messages");
    assert_eq!(coordinator.state.status.message.status_line(), "warned");
    assert!(coordinator.render_needed());

    // -- Setup & Fixtures: a busy coordinator.
    let mut erroring = RuntimeCoordinator::new(120, 40, Some(false));
    erroring.state.phase = UiPhase::Busy;
    erroring.state.input_locked = true;

    // -- Exec
    let handled = erroring.reduce_system_event(SystemEvent::SystemMessage {
        kind: SystemKind::TurnError,
        text: "boom".to_string(),
    });

    // -- Check
    assert!(handled, "TurnError must fall through and be handled");
    let error_line = erroring.state.transcript.entries().iter().any(|entry| {
        entry_to_role(entry) == TranscriptRole::System && entry_text(entry).contains("Error: boom")
    });
    assert!(error_line, "TurnError must land on the transcript");
    assert_eq!(erroring.state.phase, UiPhase::Idle);
    assert!(!erroring.state.input_locked);
    Ok(())
}

/// Status-owned UiStateEvents route through StatusState; non-status variants
/// fall back to AppState.
#[test]
fn reduce_ui_state_event_routing() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec & Check: status variants.
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
        state: agnt_core::protocol::contracts::McpUsabilityState::Failed,
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

    // -- Setup & Fixtures: a fresh coordinator.
    let mut fallback = RuntimeCoordinator::new(120, 40, Some(false));
    assert!(fallback.state.transcript.entries().is_empty());

    // -- Exec
    fallback.reduce_ui_state_event(UiStateEvent::PushStartupLogo);

    // -- Check: non-status variants fall through to AppState.
    let has_logo = fallback
        .state
        .transcript
        .entries()
        .iter()
        .any(|entry| matches!(entry, TranscriptEntry::Banner(_)));
    assert!(has_logo);
    Ok(())
}

/// A PermissionRequested SystemEvent applies all effects and marks the frame
/// dirty; decision variants skip effects; a pre-authorize display is applied
/// before the prompt opens.
#[test]
fn ui_event_permission_group() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);
    coordinator.state.status.message.set_message("idle");
    coordinator.state.scroll.following_tail = false;
    let event = SystemEvent::PermissionRequested {
        request_id: RequestId::new("ask-0000000000000001"),
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
    let handled = coordinator.reduce_system_event(event);
    if handled {
        coordinator.mark_render_needed();
    }

    // -- Check
    assert!(handled, "PermissionRequested must reduce to true");
    assert_eq!(coordinator.state.status.message.status_line(), "idle");
    assert!(coordinator.state.scroll.following_tail);
    assert!(coordinator.render_needed());
    assert!(coordinator.state.permission.has_prompt());

    // -- Exec & Check: decision variants skip effects.
    for event in [
        SystemEvent::PermissionDecisionSubmitted {
            request_id: RequestId::new("ask-0000000000000001"),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "nested:nu.command:*".to_string(),
        },
        SystemEvent::PermissionDecisionTimedOut {
            request_id: RequestId::new("ask-0000000000000001"),
        },
        SystemEvent::PermissionDecisionIgnored {
            request_id: RequestId::new("ask-0000000000000001"),
            reason: "user closed".to_string(),
        },
        SystemEvent::PermissionRequestCancelled {
            request_id: RequestId::new("ask-0000000000000001"),
            matched_rule_identity: "nested:nu.command:*".to_string(),
        },
        SystemEvent::PermissionAutoResolved {
            matched_rule_identity: "tool:read".to_string(),
            action: AutoResolutionAction::Allow,
            reason: None,
        },
    ] {
        let mut decision = RuntimeCoordinator::new(120, 40, Some(false));
        decision.set_render_needed(false);
        decision.state.status.message.set_message("idle");
        decision.state.scroll.following_tail = false;

        let handled = decision.reduce_system_event(event);
        if handled {
            decision.mark_render_needed();
        }

        assert!(!handled, "decision variants must reduce to false");
        assert_eq!(decision.state.status.message.status_line(), "idle");
        assert!(!decision.state.scroll.following_tail);
        assert!(!decision.render_needed());
    }

    // -- Setup & Fixtures: a pending tool call for the pre-authorize display.
    let mut preview = RuntimeCoordinator::new(120, 40, Some(false));
    preview.reduce_system_event(SystemEvent::ToolStarted {
        call: agnt_core::protocol::events::ToolCall::new(
            agnt_core::transcript::ir::ToolName("write".to_string()),
            "user",
            "{}",
            agnt_core::protocol::tool_args::CallLine::from_json_summary("{}"),
        ),
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
            title: Some("preview-title".to_string()),
            sections: vec![],
        }),
    };

    // -- Exec
    let handled = preview.reduce_system_event(SystemEvent::PermissionRequested {
        request_id: RequestId::new("ask-0000000000000001"),
        context,
    });

    // -- Check: the display is set on the entry's preview before the prompt
    // opens.
    assert!(handled, "PermissionRequested must reduce to true");
    let preview_pushed = preview.state.transcript.entries().iter().any(|entry| {
        matches!(
            entry,
            agnt_core::transcript::entry::TranscriptEntry::Tool(tool)
                if tool.preview.is_some()
        )
    });
    assert!(preview_pushed);
    assert!(preview.state.permission.has_prompt());
    Ok(())
}
