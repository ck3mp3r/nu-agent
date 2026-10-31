use std::time::{Duration, Instant};

use crate::interaction::reducer::reduce;
use crate::state::*;
use agnt_core::orchestrator::UiStateEvent;
use agnt_core::protocol::contracts::McpUsabilityState;
use agnt_core::protocol::events::{SystemEvent, SystemKind};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn reduce_ui_state_event_updates_status_fields() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check: model identity.
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetActiveModelIdentity("gpt-4".to_string()))
    );
    assert_eq!(state.status.identity.active_model_identity, "gpt-4");

    // -- Exec & Check: persona icon.
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetActivePersonaIcon(Some("🦀".to_string())))
    );
    assert_eq!(
        state.status.identity.active_persona_icon.as_deref(),
        Some("🦀")
    );

    // -- Exec & Check: context window max tokens.
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetContextWindowMaxTokens(Some(128_000)))
    );
    assert_eq!(state.status.tokens.context_window_max_tokens, Some(128_000));

    // -- Exec & Check: mcp server state and visible tool count.
    state.status.mcp.set_mcp_servers(vec![McpServerState {
        name: "server-a".to_string(),
        state: McpServerUsabilityState::Disabled,
    }]);
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetMcpServerState {
                server: "server-a".to_string(),
                state: McpUsabilityState::Enabled,
                error: None,
                total: 3,
            })
    );
    assert_eq!(
        state.status.mcp.mcp_servers[0].state,
        McpServerUsabilityState::Enabled
    );
    assert_eq!(state.status.mcp.llm_visible_mcp_tool_count, 3);

    // -- Exec & Check: per-server visible tool count.
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetMcpVisibleToolCount {
                server: "server-a".to_string(),
                count: 5,
            })
    );
    assert_eq!(
        state
            .status
            .mcp
            .mcp_visible_tool_count_for_server_name("server-a"),
        5
    );

    // -- Exec & Check: per-server visible tool names are sorted.
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetMcpVisibleToolNames {
                server: "server-a".to_string(),
                names: vec!["b".to_string(), "a".to_string()],
            })
    );
    assert_eq!(
        state
            .status
            .mcp
            .mcp_visible_tool_names_for_server_name("server-a"),
        vec!["a".to_string(), "b".to_string()]
    );

    // -- Exec & Check: an unhandled event reports false.
    assert!(
        !state
            .status
            .reduce_ui_state_event(UiStateEvent::ClearTranscript)
    );
    Ok(())
}

#[test]
fn status_message_kind_and_warning_events() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check: a warning system message sets the line and warning kind.
    assert!(reduce(
        &mut state,
        SystemEvent::SystemMessage {
            kind: SystemKind::Warning,
            text: "hello".to_string(),
        },
        None,
    ));
    assert_eq!(state.status.message.status_line(), "hello");
    assert_eq!(state.status.message.kind(), StatusMessageKind::Warning);

    // -- Exec & Check: set_message resets the kind to neutral.
    state.status.message.set_message("info");
    assert_eq!(state.status.message.kind(), StatusMessageKind::Neutral);

    // -- Exec & Check: clear empties the line and resets the kind.
    state.status.message.set_warning("warned");
    state.status.message.clear();
    assert!(state.status.message.status_line().is_empty());
    assert_eq!(state.status.message.kind(), StatusMessageKind::Neutral);
    Ok(())
}

#[test]
fn status_message_set_notice_sets_notice_kind_and_never_expires() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    state.status.message.set_notice("Turn stopped");

    // -- Check: the kind is Notice and the message is pending.
    assert_eq!(state.status.message.kind(), StatusMessageKind::Notice);
    assert_eq!(state.status.message.status_line(), "Turn stopped");
    assert!(state.status.message.is_pending());

    // -- Check: a Notice never expires on the clock — not at the neutral TTL,
    // not far in the future. The next turn start clears it instead.
    assert!(!state.status.message.expired(Instant::now()));
    assert!(
        !state
            .status
            .message
            .expired(Instant::now() + Duration::from_secs(5))
    );
    assert!(
        !state
            .status
            .message
            .expired(Instant::now() + Duration::from_secs(3600))
    );
    Ok(())
}

#[test]
fn status_message_expiry_matrix() -> Result<()> {
    // -- Setup & Fixtures
    let far_future = Instant::now() + Duration::from_secs(3600);

    // -- Exec & Check: an unstamped message never expires.
    let state = AppState::default();
    assert!(!state.status.message.expired(far_future));

    // -- Exec & Check: a neutral message expires after the neutral TTL.
    let mut neutral = AppState::default();
    neutral.status.message.set_message("info");
    assert!(!neutral.status.message.expired(Instant::now()));
    assert!(
        neutral
            .status
            .message
            .expired(Instant::now() + Duration::from_secs(5))
    );

    // -- Exec & Check: a warning expires after the warning TTL.
    let mut warning = AppState::default();
    warning.status.message.set_warning("warned");
    assert!(
        !warning
            .status
            .message
            .expired(Instant::now() + Duration::from_secs(5))
    );
    assert!(
        warning
            .status
            .message
            .expired(Instant::now() + Duration::from_secs(15))
    );

    // -- Exec & Check: clear resets written_at so the message never expires.
    let mut cleared = AppState::default();
    cleared.status.message.set_message("info");
    cleared.status.message.clear();
    assert!(!cleared.status.message.expired(far_future));

    // -- Exec & Check: a `now` before written_at reports false, not panic.
    let mut past = AppState::default();
    let before = Instant::now() - Duration::from_secs(1);
    past.status.message.set_message("info");
    assert!(!past.status.message.expired(before));
    Ok(())
}

#[test]
fn status_message_pending_lifecycle() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check: not pending by default.
    assert!(!state.status.message.is_pending());

    // -- Exec & Check: pending after set_message.
    state.status.message.set_message("info");
    assert!(state.status.message.is_pending());

    // -- Exec & Check: pending after set_warning.
    state.status.message.set_warning("warned");
    assert!(state.status.message.is_pending());

    // -- Exec & Check: not pending after clear.
    state.status.message.clear();
    assert!(!state.status.message.is_pending());
    Ok(())
}
