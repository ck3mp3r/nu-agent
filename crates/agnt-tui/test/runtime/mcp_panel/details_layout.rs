use super::*;

/// `mcp_details_height_for_inner_height` follows the step table for inner
/// heights 0..=20.
#[test]
fn mcp_details_height_step_table() -> Result<()> {
    // -- Setup & Fixtures
    let expected: [(u16, u16); 21] = [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 0),
        (5, 1),
        (6, 2),
        (7, 2),
        (8, 3),
        (9, 3),
        (10, 4),
        (11, 4),
        (12, 5),
        (13, 5),
        (14, 6),
        (15, 6),
        (16, 6),
        (17, 6),
        (18, 6),
        (19, 6),
        (20, 6),
    ];

    // -- Exec & Check
    for (h, want) in expected {
        assert_eq!(
            super::mcp_details_height_for_inner_height(h),
            want,
            "inner_height={h}"
        );
    }
    Ok(())
}

/// The MCP panel keeps the table primary with multiple visible rows, its
/// controls line is compact, and the visible tool count uses the live
/// per-server mapping.
#[test]
fn mcp_panel_layout_controls_and_visible_count() -> Result<()> {
    // -- Setup & Fixtures: eight servers.
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(
        (0..8)
            .map(|idx| crate::state::McpServerState {
                name: format!("srv-{idx}"),
                state: if idx % 2 == 0 {
                    McpServerUsabilityState::Enabled
                } else {
                    McpServerUsabilityState::Disabled
                },
            })
            .collect(),
    );

    // -- Exec
    let inner_height = 8u16;
    let details_height = super::mcp_details_height_for_inner_height(inner_height);
    let table_height = inner_height
        .saturating_sub(1)
        .saturating_sub(details_height);
    let model = mcp_table_model_for_test(&state, 80, table_height);

    // -- Check: multiple rows are visible at a common modal height.
    assert!(model.rows.len() > 1);

    // -- Exec & Check: the controls line documents navigation and filtering.
    let line = super::mcp_panel_controls_line();
    assert_eq!(
        line,
        "↑/↓ select · type to filter · Enter/Space toggle · Esc close"
    );
    assert!(!line.contains("enabled"));
    assert!(!line.contains("disabled"));
    assert!(!line.contains("failed"));

    // -- Setup & Fixtures: live per-server tool counts.
    let mut counted = AppState::default();
    counted.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    counted
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("gh", 4);
    counted
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("k8s", 2);

    // -- Exec & Check: the visible tool count uses the live mapping.
    let model = mcp_table_model_for_test(&counted, 80, 10);
    assert_eq!(model.rows[0][1], "4");
    assert_eq!(model.rows[1][1], "2");
    Ok(())
}
