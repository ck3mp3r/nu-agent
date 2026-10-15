use super::*;

/// The selected MCP details model shows the server line, full error text, and
/// the sorted tools list (or None); the table's visible tool count uses the
/// live per-server mapping.
#[test]
fn mcp_selected_details_model_and_visible_count() -> Result<()> {
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
    let reason = "connection timeout while dialing 10.0.0.1:443".to_string();
    state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some(reason.clone()),
    );
    state.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "k8s",
        vec!["k8s__z_last".to_string(), "k8s__a_first".to_string()],
    );

    // -- Exec & Check: the failed server's details.
    state.status.mcp.mcp_panel_selection = 1;
    let failed =
        super::mcp_selected_details_for_test(&state).ok_or("should have selected MCP details")?;
    assert_eq!(failed.server_line, "Server: k8s (failed)");
    assert_eq!(failed.error_line, format!("Error: {reason}"));
    assert_eq!(failed.tools_line, "Tools: k8s__a_first, k8s__z_last");

    // -- Exec & Check: the healthy server's details.
    state.status.mcp.mcp_panel_selection = 0;
    let healthy =
        super::mcp_selected_details_for_test(&state).ok_or("should have selected MCP details")?;
    assert_eq!(healthy.server_line, "Server: gh (enabled)");
    assert_eq!(healthy.error_line, "Error: None");
    assert_eq!(healthy.tools_line, "Tools: None");

    // -- Setup & Fixtures: three servers with live tool counts.
    let mut counted = AppState::default();
    counted.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::state::McpServerState {
            name: "docs".to_string(),
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
    counted
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("docs", 7);

    // -- Exec & Check: the table respects live updates.
    let model = mcp_table_model_for_test(&counted, 80, 10);
    assert_eq!(model.rows[0][1], "4");
    assert_eq!(model.rows[1][1], "2");
    assert_eq!(model.rows[2][1], "7");
    Ok(())
}

/// The selected details height budget preserves the error line: zero rows
/// render nothing, one row keeps the server+error, two rows keep the full
/// error, and the normal height keeps all three rows.
#[test]
fn mcp_selected_details_height_budget() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some("connection timeout while dialing 10.0.0.1:443".to_string()),
    );

    // -- Exec & Check: zero rows render nothing.
    let zero_rows = super::mcp_selected_details_lines_for_test(&state, 0, 80);
    assert!(zero_rows.is_empty());

    // -- Exec & Check: one row keeps the server+error.
    let one_row = super::mcp_selected_details_lines_for_test(&state, 1, 80);
    assert_eq!(one_row.len(), 1);
    assert!(one_row[0].contains("Error: connection timeout while dialing 10.0.0.1:443"));
    assert!(one_row[0].contains("Server: k8s (failed)"));

    // -- Setup & Fixtures: a longer error.
    let mut long = AppState::default();
    long.status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    long.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some("connection timeout while dialing 10.0.0.1:443 after many retries and additional context".to_string()),
    );

    // -- Exec & Check: two rows keep the full error line.
    let two_rows = super::mcp_selected_details_lines_for_test(&long, 2, 80);
    assert_eq!(two_rows.len(), 2);
    assert_eq!(two_rows[0], "Server: k8s (failed)");
    assert_eq!(
        two_rows[1],
        "Error: connection timeout while dialing 10.0.0.1:443 after many retries and additional context"
    );

    // -- Exec & Check: the normal height keeps all three rows.
    let full = super::mcp_selected_details_lines_for_test(&long, 3, 80);
    assert_eq!(full.len(), 3);
    assert_eq!(full[0], "Server: k8s (failed)");
    assert_eq!(
        full[1],
        "Error: connection timeout while dialing 10.0.0.1:443 after many retries and additional context"
    );
    assert_eq!(full[2], "Tools: None");
    Ok(())
}

/// The selected details tool list packs multiple tools per line with comma
/// separators, shows a deterministic "+N more" cue when clipped, aligns
/// continuation rows, and wraps using the actual details width.
#[test]
fn mcp_selected_details_tool_list_layout() -> Result<()> {
    // -- Setup & Fixtures: three tools.
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    state.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "gh",
        vec![
            "gh__z_last".to_string(),
            "gh__a_first".to_string(),
            "gh__m_mid".to_string(),
        ],
    );

    // -- Exec & Check: packing with comma separators.
    let details = super::mcp_selected_details_lines_for_test(&state, 6, 36);
    assert_eq!(
        details,
        vec![
            "Server: gh (enabled)",
            "Error: None",
            "Tools: gh__a_first, gh__m_mid",
            "       gh__z_last",
        ]
    );

    // -- Setup & Fixtures: five tools.
    let mut five = AppState::default();
    five.status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    five.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "gh",
        vec![
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
            "gh__d_fourth".to_string(),
            "gh__e_fifth".to_string(),
        ],
    );

    // -- Exec & Check: the clipped list shows the "+1 more" cue.
    let clipped = super::mcp_selected_details_lines_for_test(&five, 6, 30);
    assert_eq!(
        clipped,
        vec![
            "Server: gh (enabled)",
            "Error: None",
            "Tools: gh__a_first",
            "       gh__b_second",
            "       gh__c_third",
            "       gh__d_fourth, +1 more",
        ]
    );

    // -- Exec & Check: continuation rows align and use the Tools prefix once.
    assert_eq!(clipped[2], "Tools: gh__a_first");
    assert_eq!(clipped[3], "       gh__b_second");
    assert_eq!(clipped[4], "       gh__c_third");
    assert_eq!(clipped[5], "       gh__d_fourth, +1 more");

    // -- Setup & Fixtures: three tools for the single-row budget.
    let mut three = AppState::default();
    three
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    three.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "gh",
        vec![
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
        ],
    );

    // -- Exec & Check: a single tool row prefers the truncation cue.
    let single = super::mcp_selected_details_lines_for_test(&three, 3, 36);
    assert_eq!(single.len(), 3);
    assert_eq!(
        single,
        vec![
            "Server: gh (enabled)",
            "Error: None",
            "Tools: gh__a_first, +2 more",
        ]
    );

    // -- Setup & Fixtures: four tools for the width test.
    let mut four = AppState::default();
    four.status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    four.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "gh",
        vec![
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
            "gh__d_fourth".to_string(),
        ],
    );

    // -- Exec & Check: wrapping uses the actual details width.
    let wide = super::mcp_selected_details_lines_for_test(&four, 6, 44);
    let narrow = super::mcp_selected_details_lines_for_test(&four, 6, 24);
    assert_eq!(wide[2], "Tools: gh__a_first, gh__b_second");
    assert_eq!(wide[3], "       gh__c_third, gh__d_fourth");
    assert_eq!(narrow[2], "Tools: gh__a_first");
    assert_eq!(narrow[3], "       gh__b_second");
    assert_eq!(narrow[4], "       gh__c_third");
    Ok(())
}
