use super::*;

use agnt_render::transcript::entry::TranscriptEntry;

use crate::state::InfoPanel;
use crate::test_support::{system_message, tool_started};
use agnt_runtime::bus::{ControlCommand, FactRecord, McpServerUsability, SharedUiAction};
use agnt_runtime::protocol::events::{
    AutoResolution, NoticeKind, PermissionRecord, RequestId, SessionRecord,
};

/// A warning narration sets the status line and marks the frame dirty; a
/// TurnError falls through to the transcript and finalizes the cycle.
#[test]
fn ui_event_warning_and_turn_error() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);

    // -- Exec
    let handled = coordinator.fold_narration(&system_message(NoticeKind::Warning, "warned"));
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
    let handled = erroring.fold_narration(&system_message(NoticeKind::TurnError, "boom"));

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

/// Facts fold through `StatusState`; UI-state events stay with `AppState`.
#[test]
fn reduce_fact_record_routing() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec & Check: the kernel-state families.
    assert!(
        coordinator
            .reduce_fact_record(FactRecord::ActiveModelIdentity("openai/gpt-4o".to_string(),))
    );
    coordinator.mark_render_needed();
    assert_eq!(
        coordinator.state.status.identity.active_model_identity,
        "openai/gpt-4o"
    );
    assert!(coordinator.render_needed());

    assert!(
        coordinator.reduce_fact_record(FactRecord::ActivePersonaIcon(Some("icon".to_string())))
    );
    assert_eq!(
        coordinator.state.status.identity.active_persona_icon,
        Some("icon".to_string())
    );

    assert!(coordinator.reduce_fact_record(FactRecord::ContextWindowMaxTokens(Some(128_000))));
    assert_eq!(
        coordinator.state.status.tokens.context_window_max_tokens,
        Some(128_000)
    );

    assert!(coordinator.reduce_fact_record(FactRecord::McpServerState {
        server: "gh".to_string(),
        state: McpServerUsability::Failed,
        error: Some("boom".to_string()),
    }));
    assert!(coordinator.reduce_fact_record(FactRecord::McpVisibleToolTotal(3)));
    assert_eq!(coordinator.state.status.mcp.llm_visible_mcp_tool_count, 3);

    assert!(
        coordinator.reduce_fact_record(FactRecord::McpVisibleToolCount {
            server: "gh".to_string(),
            count: 5,
        })
    );
    assert_eq!(
        coordinator
            .state
            .status
            .mcp
            .mcp_visible_tool_count_for_server_name("gh"),
        5
    );

    assert!(
        coordinator.reduce_fact_record(FactRecord::McpVisibleToolNames {
            server: "gh".to_string(),
            names: vec!["z_tool".to_string(), "a_tool".to_string()],
        })
    );
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
    fallback.state.push_startup_logo();

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

/// A permission request applies all effects and marks the frame dirty;
/// resolution records skip effects; a pre-authorize display is applied before
/// the prompt opens.
#[test]
fn ui_event_permission_group() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);
    coordinator.state.status.message.set_message("idle");
    coordinator.state.scroll.following_tail = false;
    let event = SessionRecord::Permission(PermissionRecord::Requested {
        request_id: RequestId::from_minted("ask-0000000000000001"),
        call_id: agnt_runtime::protocol::events::ToolCallId::from_key(
            "nu\n{\"command\":\"echo hi\"}",
        ),
        tool: "nu(command=echo hi)".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        rule_identity: "nested:nu.command:*".to_string(),
        scope: "nested".to_string(),
        target_field: Some("command".to_string()),
        pattern: "*".to_string(),
        summary: "→ {\"command\":\"echo hi\"}".to_string(),
        shows_preview: true,
        preview: None,
    });

    // -- Exec
    let handled = coordinator.fold_record(&event);
    if handled {
        coordinator.mark_render_needed();
    }

    // -- Check
    assert!(handled, "PermissionRequested must reduce to true");
    assert_eq!(coordinator.state.status.message.status_line(), "idle");
    assert!(coordinator.state.scroll.following_tail);
    assert!(coordinator.render_needed());
    assert!(coordinator.state.permission.has_prompt());

    // -- Exec & Check: resolution records skip effects.
    for event in [
        SessionRecord::Permission(PermissionRecord::Decided {
            request_id: RequestId::from_minted("ask-0000000000000001"),
            decision: PermissionDecision::AllowOnce,
            rule_identity: "nested:nu.command:*".to_string(),
        }),
        SessionRecord::Permission(PermissionRecord::TimedOut {
            request_id: RequestId::from_minted("ask-0000000000000001"),
        }),
        SessionRecord::Permission(PermissionRecord::Ignored {
            request_id: RequestId::from_minted("ask-0000000000000001"),
            reason: "user closed".to_string(),
        }),
        SessionRecord::Permission(PermissionRecord::Cancelled {
            request_id: RequestId::from_minted("ask-0000000000000001"),
            rule_identity: "nested:nu.command:*".to_string(),
        }),
        SessionRecord::Permission(PermissionRecord::AutoResolved {
            rule_identity: "tool:read".to_string(),
            action: AutoResolution::Allow,
            reason: None,
            call_id: None,
            shows_preview: false,
            preview: None,
        }),
    ] {
        let mut decision = RuntimeCoordinator::new(120, 40, Some(false));
        decision.set_render_needed(false);
        decision.state.status.message.set_message("idle");
        decision.state.scroll.following_tail = false;

        let handled = decision.fold_record(&event);
        if handled {
            decision.mark_render_needed();
        }

        assert!(!handled, "resolution records must reduce to false");
        assert_eq!(decision.state.status.message.status_line(), "idle");
        assert!(!decision.state.scroll.following_tail);
        assert!(!decision.render_needed());
    }

    // -- Setup & Fixtures: a pending tool call for the pre-authorize display.
    let mut preview = RuntimeCoordinator::new(120, 40, Some(false));
    preview.fold_record(&tool_started(
        agnt_runtime::protocol::events::ToolCall::new(
            agnt_render::transcript::ir::ToolName("edit".to_string()),
            agnt_runtime::tools::handler::ToolSource::Unknown,
            r#"{"path":"a.rs"}"#,
        ),
    ));
    let request = SessionRecord::Permission(PermissionRecord::Requested {
        request_id: RequestId::from_minted("ask-0000000000000001"),
        call_id: agnt_runtime::protocol::events::ToolCallId::from_key("edit\n{\"path\":\"a.rs\"}"),
        tool: "edit".to_string(),
        source: "user".to_string(),
        mode: Some("apply".to_string()),
        rule_identity: "identity".to_string(),
        scope: "scope".to_string(),
        target_field: Some("target".to_string()),
        pattern: "pattern".to_string(),
        summary: "summary".to_string(),
        shows_preview: true,
        preview: Some("--- a\n+++ b\n".to_string()),
    });

    // -- Exec
    let handled = preview.fold_record(&request);

    // -- Check: the display is set on the entry's preview before the prompt
    // opens.
    assert!(handled, "PermissionRequested must reduce to true");
    let preview_pushed = preview.state.transcript.entries().iter().any(|entry| {
        matches!(
            entry,
            agnt_render::transcript::entry::TranscriptEntry::Tool(tool)
                if tool.preview.is_some()
        )
    });
    assert!(preview_pushed);
    assert!(preview.state.permission.has_prompt());
    Ok(())
}

/// WHEN a control command folds to a state change, THE control intake SHALL
/// mark the frame dirty: the loop repaints only on the flag, and the control
/// arm publishes no marker of its own.
#[test]
fn test_runtime_render_animation_control_fold_marks_render_needed() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);

    // -- Exec: the status handoff a slash command publishes.
    let changed = coordinator.reduce_control(&ControlCommand::SharedUiAction {
        action: SharedUiAction::Status,
    });

    // -- Check: the fold opened the panel and marked the frame dirty.
    assert!(changed, "a shared UI action folds to true");
    assert_eq!(
        coordinator.state.info_panel,
        Some(InfoPanel::Status),
        "the folded action opens the panel it names"
    );
    assert!(
        coordinator.render_needed(),
        "a control fold that changed the state must mark the frame dirty"
    );

    // -- Setup & Fixtures: a control command the surface does not own.
    let mut foreign = RuntimeCoordinator::new(120, 40, Some(false));
    foreign.set_render_needed(false);

    // -- Exec
    let changed = foreign.reduce_control(&ControlCommand::CompactionRequested {
        source: agnt_runtime::protocol::events::CompactionSource::Manual,
    });

    // -- Check: a fold that changed nothing leaves the flag untouched.
    assert!(!changed, "a foreign control command folds to false");
    assert!(
        !foreign.render_needed(),
        "a control fold that changed nothing must not mark the frame dirty"
    );
    Ok(())
}
