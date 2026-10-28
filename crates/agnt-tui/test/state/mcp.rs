use crate::state::*;
use agnt_core::orchestrator::UiStateEvent;
use agnt_core::protocol::contracts::McpUsabilityState;

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
    // -- Setup & Fixtures: (initial state, request enable, state after toggle)
    let cases = [
        (
            McpServerUsabilityState::Failed,
            false,
            McpServerUsabilityState::Disabled,
        ),
        (
            McpServerUsabilityState::Enabled,
            false,
            McpServerUsabilityState::Disabled,
        ),
        (
            McpServerUsabilityState::Disabled,
            true,
            McpServerUsabilityState::Disabled,
        ),
    ];

    // -- Exec & Check
    for (initial, expected_enable, expected_state) in cases {
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
            state.status.mcp.selected_mcp_server_state(),
            Some(expected_state),
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

/// Toggling a failed server disables it; the confirming response drops the
/// failure reason, and the next toggle retries the connection.
#[test]
fn failed_server_toggle_disables_and_drops_the_failure_reason() -> Result<()> {
    // -- Setup & Fixtures: a failed server carrying a load error.
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![mcp_server("broken", McpServerUsabilityState::Failed)]);
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "broken",
        McpServerUsabilityState::Failed,
        Some("connection refused".to_string()),
    ));
    assert_eq!(
        state.status.mcp.failed_mcp_servers_with_reasons(),
        vec![("broken", Some("connection refused"))]
    );

    // -- Exec: toggling the failed row asks for a disable, not a retry.
    assert!(state.status.mcp.queue_selected_mcp_toggle_request());
    let request = state
        .status
        .mcp
        .take_next_mcp_toggle_request()
        .ok_or("should have toggle request")?;
    assert_eq!(request.server_name, "broken");
    assert!(!request.enable, "a failed server toggles off");

    // -- Check: the local row flips to Disabled immediately.
    assert_eq!(
        state.status.mcp.selected_mcp_server_state(),
        Some(McpServerUsabilityState::Disabled)
    );

    // -- Exec: the disable response confirms Disabled with no error.
    assert!(
        state
            .status
            .reduce_ui_state_event(UiStateEvent::SetMcpServerState {
                server: "broken".to_string(),
                state: McpUsabilityState::Disabled,
                error: None,
                total: 0,
            })
    );

    // -- Check: the row stays Disabled and the failure reason is gone.
    assert_eq!(
        state.status.mcp.selected_mcp_server_state(),
        Some(McpServerUsabilityState::Disabled)
    );
    assert!(
        state
            .status
            .mcp
            .failed_mcp_servers_with_reasons()
            .is_empty(),
        "disabling must drop the failure reason"
    );

    // -- Exec: toggling the now-disabled server retries the connection.
    assert!(state.status.mcp.queue_selected_mcp_toggle_request());
    let retry = state
        .status
        .mcp
        .take_next_mcp_toggle_request()
        .ok_or("should have retry request")?;
    assert!(retry.enable, "a disabled server retries on toggle");

    // -- Check: the retry leaves the local row for the runtime to confirm.
    assert_eq!(
        state.status.mcp.selected_mcp_server_state(),
        Some(McpServerUsabilityState::Disabled)
    );
    Ok(())
}

#[test]
fn mcp_panel_query_filters_and_follows_the_selection() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        mcp_server("c5t", McpServerUsabilityState::Enabled),
        mcp_server("context7", McpServerUsabilityState::Disabled),
        mcp_server("github", McpServerUsabilityState::Enabled),
    ]);

    // -- Check: an empty query keeps every row and selects the first.
    assert_eq!(state.status.mcp.mcp_panel_query(), "");
    assert_eq!(state.status.mcp.mcp_servers_filtered().len(), 3);
    assert_eq!(state.status.mcp.selected_mcp_server_name(), Some("c5t"));

    // -- Exec: a case-insensitive subsequence query.
    for ch in "C7".chars() {
        state.status.mcp.append_mcp_panel_query_char(ch);
    }

    // -- Check: only the subsequence match survives, and it is selected.
    assert_eq!(state.status.mcp.mcp_panel_query(), "C7");
    let filtered = state.status.mcp.mcp_servers_filtered();
    let names = filtered.iter().map(|s| s.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, vec!["context7"]);
    assert_eq!(
        state.status.mcp.selected_mcp_server_name(),
        Some("context7")
    );

    // -- Exec: the toggle applies to the selected row of the filtered view.
    assert!(state.status.mcp.queue_selected_mcp_toggle_request());

    // -- Check: the request names the filtered row, not the first server, and an
    // enabling request leaves the local state for the runtime to confirm.
    let request = state
        .status
        .mcp
        .take_next_mcp_toggle_request()
        .ok_or("should have toggle request")?;
    assert_eq!(request.server_name, "context7");
    assert!(request.enable, "a disabled server toggles on");
    assert_eq!(state.status.mcp.mcp_counts(), (3, 2, 1, 0));

    // -- Exec & Check: movement wraps inside the filtered rows only.
    state.status.mcp.mcp_panel_move_down();
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);
    state.status.mcp.mcp_panel_move_up();
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);

    // -- Exec & Check: Backspace widens the filter and re-clamps the selection.
    assert!(state.status.mcp.backspace_mcp_panel_query_char());
    assert_eq!(state.status.mcp.mcp_panel_query(), "C");
    assert_eq!(state.status.mcp.mcp_servers_filtered().len(), 2);
    state.status.mcp.mcp_panel_selection = 5;
    state.status.mcp.clamp_mcp_panel_selection();
    assert_eq!(state.status.mcp.mcp_panel_selection, 1);

    // -- Exec & Check: an empty query has nothing to erase.
    state.status.mcp.clear_mcp_panel_query();
    assert!(!state.status.mcp.backspace_mcp_panel_query_char());

    // -- Exec: a query that selects an enabled row.
    state.status.mcp.append_mcp_panel_query_char('g');
    assert_eq!(state.status.mcp.selected_mcp_server_name(), Some("github"));
    assert!(state.status.mcp.queue_selected_mcp_toggle_request());

    // -- Check: disabling flips the local row immediately.
    let request = state
        .status
        .mcp
        .take_next_mcp_toggle_request()
        .ok_or("should have toggle request")?;
    assert_eq!(request.server_name, "github");
    assert!(!request.enable, "an enabled server toggles off");
    assert_eq!(state.status.mcp.mcp_counts(), (3, 1, 2, 0));

    // -- Exec: a new server list invalidates the query.
    state
        .status
        .mcp
        .set_mcp_servers(vec![mcp_server("github", McpServerUsabilityState::Enabled)]);

    // -- Check
    assert_eq!(state.status.mcp.mcp_panel_query(), "");
    assert_eq!(state.status.mcp.mcp_servers_filtered().len(), 1);
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);
    Ok(())
}

/// A server row for the MCPs panel fixtures.
fn mcp_server(name: &str, state: McpServerUsabilityState) -> crate::state::McpServerState {
    crate::state::McpServerState {
        name: name.to_string(),
        state,
    }
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
