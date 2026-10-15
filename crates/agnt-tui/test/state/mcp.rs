use crate::state::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn mcp_server_toggle_and_counts_follow_selected_row() -> Result<()> {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);

    assert_eq!(state.status.mcp.mcp_counts(), (2, 1, 1, 0));

    state.status.mcp.mcp_panel_move_down();
    assert!(state.status.mcp.queue_selected_mcp_toggle_request());

    assert_eq!(state.status.mcp.mcp_counts(), (2, 1, 1, 0));
    assert_eq!(
        state.status.mcp.selected_mcp_server_state(),
        Some(McpServerUsabilityState::Disabled)
    );

    let request = state
        .status
        .mcp
        .take_next_mcp_toggle_request()
        .ok_or("should have toggle request")?;
    assert_eq!(request.server_name, "k8s");
    assert!(request.enable);

    assert!(
        state
            .status
            .mcp
            .set_mcp_server_state_by_name("k8s", McpServerUsabilityState::Enabled)
    );
    assert_eq!(state.status.mcp.mcp_counts(), (2, 2, 0, 0));
    Ok(())
}

#[test]
fn mcp_toggle_request_and_counts_follow_server_state() -> Result<()> {
    // -- Setup & Fixtures: (initial state, expected request enable, expected counts)
    let cases = [
        (McpServerUsabilityState::Failed, true, (1, 0, 0, 1)),
        (McpServerUsabilityState::Enabled, false, (1, 0, 1, 0)),
    ];

    // -- Exec & Check
    for (initial, expected_enable, expected_counts) in cases {
        let mut state = AppState::default();
        state
            .status
            .mcp
            .set_mcp_servers(vec![crate::state::McpServerState {
                name: "gh".to_string(),
                state: initial,
            }]);

        assert!(state.status.mcp.queue_selected_mcp_toggle_request());
        let request = state
            .status
            .mcp
            .take_next_mcp_toggle_request()
            .ok_or("should have toggle request")?;
        assert_eq!(request.server_name, "gh");
        assert_eq!(request.enable, expected_enable, "initial={initial:?}");
        assert_eq!(
            state.status.mcp.mcp_counts(),
            expected_counts,
            "initial={initial:?}"
        );

        // Re-marking the server failed always succeeds and keeps the counts.
        assert!(
            state
                .status
                .mcp
                .set_mcp_server_state_by_name("gh", McpServerUsabilityState::Failed)
        );
        assert_eq!(state.status.mcp.mcp_counts(), (1, 0, 0, 1));
    }
    Ok(())
}

#[test]
fn failed_server_reason_round_trips_through_state_query() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        }]);

    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some("dial tcp timeout".to_string()),
    ));

    let failed = state.status.mcp.failed_mcp_servers_with_reasons();
    assert_eq!(failed, vec![("k8s", Some("dial tcp timeout"))]);
}
