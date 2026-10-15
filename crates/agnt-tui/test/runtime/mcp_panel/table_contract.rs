use super::*;

/// The MCP table renders the required columns, selection, compact rows, and a
/// deterministic status-icon mapping with a safe status-column width.
#[test]
fn mcp_table_contract() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    state.status.mcp.mcp_panel_selection = 1;
    state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some("connect timeout".to_string()),
    );
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("gh", 3);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("k8s", 9);

    // -- Exec
    let model = mcp_table_model_for_test(&state, 80, 10);

    // -- Check: columns, selection, and rows.
    assert_eq!(model.columns, vec!["Name", "Visible tools", "Status"]);
    assert_eq!(model.selected, Some(1));
    assert_eq!(model.rows.len(), 2);
    assert_eq!(model.rows[0][0], "gh");
    assert_eq!(model.rows[0][1], "3");
    assert_eq!(model.rows[0][2], "🟢");
    assert_eq!(model.rows[1][0], "k8s");
    assert_eq!(model.rows[1][1], "9");
    assert_eq!(model.rows[1][2], "🔴");

    // -- Setup & Fixtures: one server per state.
    let mut icons = AppState::default();
    icons.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "enabled-srv".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "disabled-srv".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::state::McpServerState {
            name: "failed-srv".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);

    // -- Exec
    let model = mcp_table_model_for_test(&icons, 80, 10);

    // -- Check: the status-icon mapping and column width.
    assert_eq!(super::MCP_STATUS_COLUMN_WIDTH, 6);
    assert_eq!(model.rows[0][2], "🟢");
    assert_eq!(model.rows[1][2], "⚪");
    assert_eq!(model.rows[2][2], "🔴");
    Ok(())
}
