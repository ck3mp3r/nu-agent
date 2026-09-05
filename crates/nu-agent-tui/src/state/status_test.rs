use crate::state::*;
use nu_agent_core::bus::WarningEvent;
use nu_agent_core::orchestrator::UiStateEvent;
use nu_agent_core::protocol::contracts::McpUsabilityState;

#[test]
fn reduce_ui_state_event_sets_active_model_identity() {
    let mut state = AppState::default();
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetActiveModelIdentity("gpt-4".to_string()))
    );
    assert_eq!(state.status.identity.active_model_identity, "gpt-4");
}

#[test]
fn reduce_ui_state_event_sets_active_persona_icon() {
    let mut state = AppState::default();
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetActivePersonaIcon(Some("🦀".to_string())))
    );
    assert_eq!(
        state.status.identity.active_persona_icon.as_deref(),
        Some("🦀")
    );
}

#[test]
fn reduce_ui_state_event_sets_context_window_max_tokens() {
    let mut state = AppState::default();
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetContextWindowMaxTokens(Some(128_000)))
    );
    assert_eq!(state.status.tokens.context_window_max_tokens, Some(128_000));
}

#[test]
fn reduce_ui_state_event_sets_mcp_server_state() {
    let mut state = AppState::default();
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
}

#[test]
fn reduce_ui_state_event_sets_mcp_visible_tool_count() {
    let mut state = AppState::default();
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
}

#[test]
fn reduce_ui_state_event_sets_mcp_visible_tool_names() {
    let mut state = AppState::default();
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
}

#[test]
fn reduce_ui_state_event_returns_false_for_unhandled() {
    let mut state = AppState::default();
    assert!(
        !state
            .status
            .reduce_ui_state_event(UiStateEvent::ClearTranscript)
    );
}

#[test]
fn reduce_warning_event_message_sets_status_line() {
    let mut state = AppState::default();
    assert!(state.status.reduce_warning_event(WarningEvent::Message {
        message: "hello".to_string()
    }));
    assert_eq!(state.status.status_line, "hello");
}

#[test]
fn reduce_warning_event_turn_error_returns_false() {
    let mut state = AppState::default();
    assert!(!state.status.reduce_warning_event(WarningEvent::TurnError {
        message: "boom".to_string()
    }));
}
