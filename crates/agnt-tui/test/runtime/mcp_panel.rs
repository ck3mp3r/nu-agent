use super::*;

#[test]
fn mcp_panel_renders_columns_selection_and_compact_table_contract() {
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

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.columns, vec!["Name", "Visible tools", "Status"]);
    assert_eq!(model.selected, Some(1));
    assert_eq!(model.rows.len(), 2);
    assert_eq!(model.rows[0][0], "gh");
    assert_eq!(model.rows[0][1], "3");
    assert_eq!(model.rows[0][2], "🟢");
    assert_eq!(model.rows[1][0], "k8s");
    assert_eq!(model.rows[1][1], "9");
    assert_eq!(model.rows[1][2], "🔴");
}

#[test]
fn mcp_table_status_icon_mapping_is_deterministic() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
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

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.rows[0][2], "🟢");
    assert_eq!(model.rows[1][2], "⚪");
    assert_eq!(model.rows[2][2], "🔴");
}

#[test]
fn mcp_table_emoji_status_rows_use_safe_status_column_width_contract() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
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

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(super::MCP_STATUS_COLUMN_WIDTH, 6);
    assert_eq!(model.rows[0][2], "🟢");
    assert_eq!(model.rows[1][2], "⚪");
    assert_eq!(model.rows[2][2], "🔴");
}

#[test]
fn mcp_details_height_allocation_prefers_multiple_tool_lines_in_normal_popup_heights() {
    assert_eq!(super::mcp_details_height_for_inner_height(4), 0);
    assert_eq!(super::mcp_details_height_for_inner_height(5), 1);
    assert_eq!(super::mcp_details_height_for_inner_height(6), 2);
    assert_eq!(super::mcp_details_height_for_inner_height(8), 3);
    assert_eq!(super::mcp_details_height_for_inner_height(10), 4);
    assert_eq!(super::mcp_details_height_for_inner_height(12), 5);
    assert_eq!(super::mcp_details_height_for_inner_height(14), 6);
}

#[test]
fn mcp_details_height_formula_matches_step_table() {
    // Verifies all inner_height values 0..=20 against the expected step-table output.
    // The step-table was replaced by a match expression for idiomatic clarity;
    // this test pins the observable behaviour so any future change is caught.
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
    for (h, want) in expected {
        assert_eq!(
            super::mcp_details_height_for_inner_height(h),
            want,
            "inner_height={h}"
        );
    }
}

#[test]
fn mcp_panel_layout_keeps_table_primary_with_multiple_visible_rows_in_common_height() {
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

    let inner_height = 8u16;
    let details_height = super::mcp_details_height_for_inner_height(inner_height);
    let table_height = inner_height
        .saturating_sub(1)
        .saturating_sub(details_height);
    let model = mcp_table_model_for_test(&state, 80, table_height);

    assert!(
        model.rows.len() > 1,
        "table should show multiple rows at common modal height"
    );
}

#[test]
fn mcp_panel_controls_line_removes_status_legend_and_keeps_toggle_hint_compact() {
    let line = super::mcp_panel_controls_line();
    assert_eq!(
        line,
        "Session-only toggles | Enter/Space toggle | Esc close"
    );
    assert!(!line.contains("enabled"));
    assert!(!line.contains("disabled"));
    assert!(!line.contains("failed"));
}

#[test]
fn mcp_table_visible_tool_count_uses_live_per_server_mapping_without_state_gating() {
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
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("gh", 4);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("k8s", 2);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.rows[0][1], "4");
    assert_eq!(model.rows[1][1], "2");
}

#[test]
fn mcp_selected_details_model_shows_full_error_text_tools_list_and_fallback() -> Result<()> {
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

    state.status.mcp.mcp_panel_selection = 1;
    let failed =
        super::mcp_selected_details_for_test(&state).ok_or("should have selected MCP details")?;
    assert_eq!(failed.server_line, "Server: k8s (failed)");
    assert_eq!(failed.error_line, format!("Error: {reason}"));
    assert_eq!(failed.tools_line, "Tools: k8s__a_first, k8s__z_last");

    state.status.mcp.mcp_panel_selection = 0;
    let healthy =
        super::mcp_selected_details_for_test(&state).ok_or("should have selected MCP details")?;
    assert_eq!(healthy.server_line, "Server: gh (enabled)");
    assert_eq!(healthy.error_line, "Error: None");
    assert_eq!(healthy.tools_line, "Tools: None");
    Ok(())
}

#[test]
fn mcp_table_visible_tool_count_respects_live_updates_after_selection_changes() {
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
        crate::state::McpServerState {
            name: "docs".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("gh", 4);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("k8s", 2);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("docs", 7);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.rows[0][1], "4");
    assert_eq!(model.rows[1][1], "2");
    assert_eq!(model.rows[2][1], "7");
}

#[test]
fn mcp_selected_details_height_zero_and_one_rows_preserve_error_presence() {
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

    let zero_rows = super::mcp_selected_details_lines_for_test(&state, 0, 80);
    assert!(zero_rows.is_empty());

    let one_row = super::mcp_selected_details_lines_for_test(&state, 1, 80);
    assert_eq!(one_row.len(), 1);
    assert!(one_row[0].contains("Error: connection timeout while dialing 10.0.0.1:443"));
    assert!(one_row[0].contains("Server: k8s (failed)"));
}

#[test]
fn mcp_selected_details_constrained_two_rows_preserve_full_error_line() {
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
        Some("connection timeout while dialing 10.0.0.1:443 after many retries and additional context".to_string()),
    );

    let two_rows = super::mcp_selected_details_lines_for_test(&state, 2, 80);
    assert_eq!(two_rows.len(), 2);
    assert_eq!(two_rows[0], "Server: k8s (failed)");
    assert_eq!(
        two_rows[1],
        "Error: connection timeout while dialing 10.0.0.1:443 after many retries and additional context"
    );
}

#[test]
fn mcp_selected_details_normal_height_preserves_full_error_line() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    let reason =
        "connection timeout while dialing 10.0.0.1:443 after many retries and additional context";
    state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some(reason.to_string()),
    );

    let full = super::mcp_selected_details_lines_for_test(&state, 3, 80);
    assert_eq!(full.len(), 3);
    assert_eq!(full[0], "Server: k8s (failed)");
    assert_eq!(full[1], format!("Error: {reason}"));
    assert_eq!(full[2], "Tools: None");
}

#[test]
fn mcp_selected_details_packs_multiple_tools_per_line_with_comma_separators() {
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
}

#[test]
fn mcp_selected_details_clipped_tool_list_shows_deterministic_plus_n_more_cue() {
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
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
            "gh__d_fourth".to_string(),
            "gh__e_fifth".to_string(),
        ],
    );

    let details = super::mcp_selected_details_lines_for_test(&state, 6, 30);
    assert_eq!(
        details,
        vec![
            "Server: gh (enabled)",
            "Error: None",
            "Tools: gh__a_first",
            "       gh__b_second",
            "       gh__c_third",
            "       gh__d_fourth, +1 more",
        ]
    );
}

#[test]
fn mcp_selected_details_single_tool_row_budget_prefers_truncation_cue_visibility() {
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
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
        ],
    );

    let details = super::mcp_selected_details_lines_for_test(&state, 3, 36);
    assert_eq!(details.len(), 3);
    assert_eq!(
        details,
        vec![
            "Server: gh (enabled)",
            "Error: None",
            "Tools: gh__a_first, +2 more",
        ]
    );
}

#[test]
fn mcp_selected_details_continuation_rows_align_and_use_tools_prefix_once() {
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
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
            "gh__d_fourth".to_string(),
            "gh__e_fifth".to_string(),
        ],
    );

    let details = super::mcp_selected_details_lines_for_test(&state, 6, 30);
    assert_eq!(details[2], "Tools: gh__a_first");
    assert_eq!(details[3], "       gh__b_second");
    assert_eq!(details[4], "       gh__c_third");
    assert_eq!(details[5], "       gh__d_fourth, +1 more");
}

#[test]
fn mcp_selected_details_wrapping_uses_actual_details_width() {
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
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
            "gh__d_fourth".to_string(),
        ],
    );

    let wide = super::mcp_selected_details_lines_for_test(&state, 6, 44);
    let narrow = super::mcp_selected_details_lines_for_test(&state, 6, 24);

    assert_eq!(wide[2], "Tools: gh__a_first, gh__b_second");
    assert_eq!(wide[3], "       gh__c_third, gh__d_fourth");
    assert_eq!(narrow[2], "Tools: gh__a_first");
    assert_eq!(narrow[3], "       gh__b_second");
    assert_eq!(narrow[4], "       gh__c_third");
}

#[test]
fn mcp_table_model_narrow_width_keeps_required_columns() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);

    let model = mcp_table_model_for_test(&state, 32, 8);
    assert_eq!(model.columns, vec!["Name", "Visible tools", "Status"]);
    assert_eq!(model.rows.len(), 1);
}

#[test]
fn mcp_table_model_overflow_top_window_locks_exact_cue_and_selected_mapping() -> Result<()> {
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
    state.status.mcp.mcp_panel_selection = 0;

    let model = mcp_table_model_for_test(&state, 80, 7);
    assert_eq!(model.selected, Some(0));
    let selected = model.selected.ok_or("should have selection")?;
    assert_eq!(model.rows[selected][0], "srv-0");
    assert_eq!(
        model.overflow_cue,
        Some("↑/↓ or j/k | Enter/Space toggle | Esc close | 1-5 / 8".to_string())
    );
    Ok(())
}

#[test]
fn mcp_table_model_overflow_middle_window_locks_exact_cue_and_selected_mapping() -> Result<()> {
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
    state.status.mcp.mcp_panel_selection = 5;

    let model = mcp_table_model_for_test(&state, 80, 7);
    assert_eq!(model.selected, Some(4));
    let selected = model.selected.ok_or("should have selection")?;
    assert_eq!(model.rows[selected][0], "srv-5");
    assert_eq!(
        model.overflow_cue,
        Some("↑/↓ or j/k | Enter/Space toggle | Esc close | 2-6 / 8".to_string())
    );
    Ok(())
}

#[test]
fn mcp_table_model_overflow_bottom_window_locks_exact_cue_and_selected_mapping() -> Result<()> {
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
    state.status.mcp.mcp_panel_selection = 7;

    let model = mcp_table_model_for_test(&state, 80, 7);
    assert_eq!(model.selected, Some(4));
    let selected = model.selected.ok_or("should have selection")?;
    assert_eq!(model.rows[selected][0], "srv-7");
    assert_eq!(
        model.overflow_cue,
        Some("↑/↓ or j/k | Enter/Space toggle | Esc close | 4-8 / 8".to_string())
    );
    Ok(())
}
