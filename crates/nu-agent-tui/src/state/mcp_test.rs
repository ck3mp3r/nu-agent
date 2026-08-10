use crate::state::*;

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
