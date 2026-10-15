use super::*;

#[test]
fn esc_closes_mcps_panel_and_preserves_insert_mode() {
    let mut state = AppState::default();
    state.open_info_panel(InfoPanel::Mcps);
    assert_eq!(state.input.mode, InputMode::Insert);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_eq!(state.info_panel, None);
    assert_eq!(state.input.mode, InputMode::Insert);
}

#[test]
fn mcp_panel_navigation_table() {
    let cases = [
        (TerminalKey::CtrlN, 0usize, 1usize),
        (TerminalKey::CtrlP, 1usize, 0usize),
        (TerminalKey::Char('j'), 0usize, 0usize),
    ];
    for (key, start, expected) in cases {
        let mut state = AppState::default();
        state.status.mcp.set_mcp_servers(vec![
            McpServerState {
                name: "gh".to_string(),
                state: McpServerUsabilityState::Enabled,
            },
            McpServerState {
                name: "k8s".to_string(),
                state: McpServerUsabilityState::Disabled,
            },
        ]);
        state.open_info_panel(InfoPanel::Mcps);
        state.status.mcp.mcp_panel_selection = start;

        let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(key), None);

        assert_eq!(state.status.mcp.mcp_panel_selection, expected);
    }
}

#[test]
fn mcps_panel_toggle_table() -> Result<()> {
    // Navigation and Enter toggle the selected server.
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    state.open_info_panel(InfoPanel::Mcps);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.status.mcp.mcp_panel_selection, 1);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(
        state.status.mcp.mcp_servers[1].state,
        McpServerUsabilityState::Disabled,
        "enable is async; state remains disabled until runtime applies result"
    );

    let request = state
        .status
        .mcp
        .take_next_mcp_toggle_request()
        .ok_or("should have queued toggle request")?;
    assert_eq!(request.server_name, "k8s");
    assert!(request.enable);

    // Up, Ctrl-P and Space toggle.
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    state.open_info_panel(InfoPanel::Mcps);
    state.status.mcp.mcp_panel_selection = 1;

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Up), None);
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);

    // Ctrl-P moves selection up (wraps: 0 -> len-1 = 1)
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    assert_eq!(state.status.mcp.mcp_panel_selection, 1);

    let _ = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char(' ')),
        None,
    );
    let request = state
        .status
        .mcp
        .take_next_mcp_toggle_request()
        .ok_or("should have queued toggle request")?;
    assert_eq!(request.server_name, "k8s");
    assert!(request.enable);
    Ok(())
}
