use super::*;

/// The MCP table model keeps the required columns at a narrow width.
#[test]
fn mcp_table_model_narrow_width_keeps_required_columns() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);

    // -- Exec
    let model = mcp_table_model_for_test(&state, 32, 8);

    // -- Check
    assert_eq!(model.columns, vec!["Name", "Visible tools", "Status"]);
    assert_eq!(model.rows.len(), 1);
    Ok(())
}

/// The MCP table model locks the exact overflow cue and selected mapping for
/// top, middle, and bottom windows.
#[test]
fn mcp_table_model_overflow_windows() -> Result<()> {
    // -- Setup & Fixtures
    let servers = || {
        (0..8)
            .map(|idx| crate::state::McpServerState {
                name: format!("srv-{idx}"),
                state: if idx % 2 == 0 {
                    McpServerUsabilityState::Enabled
                } else {
                    McpServerUsabilityState::Disabled
                },
            })
            .collect::<Vec<_>>()
    };
    let cases: [(&str, usize, usize, &str, &str); 3] = [
        (
            "top",
            0,
            0,
            "srv-0",
            "↑/↓ or j/k | Enter/Space toggle | Esc close | 1-5 / 8",
        ),
        (
            "middle",
            5,
            4,
            "srv-5",
            "↑/↓ or j/k | Enter/Space toggle | Esc close | 2-6 / 8",
        ),
        (
            "bottom",
            7,
            4,
            "srv-7",
            "↑/↓ or j/k | Enter/Space toggle | Esc close | 4-8 / 8",
        ),
    ];

    // -- Exec & Check
    for (label, selection, expected_selected, expected_name, expected_cue) in cases {
        let mut state = AppState::default();
        state.status.mcp.set_mcp_servers(servers());
        state.status.mcp.mcp_panel_selection = selection;

        let model = mcp_table_model_for_test(&state, 80, 7);
        assert_eq!(model.selected, Some(expected_selected), "{label}");
        let selected = model.selected.ok_or("should have selection")?;
        assert_eq!(model.rows[selected][0], expected_name, "{label}");
        assert_eq!(model.overflow_cue.as_deref(), Some(expected_cue), "{label}");
    }
    Ok(())
}
