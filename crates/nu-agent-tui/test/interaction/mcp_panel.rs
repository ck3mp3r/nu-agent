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
fn mcps_panel_navigation_and_enter_toggle_updates_selected_server() -> Result<()> {
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
    Ok(())
}

#[test]
fn mcps_panel_supports_up_ctrl_p_and_space_toggle() -> Result<()> {
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

#[test]
fn mcp_panel_ctrl_n_moves_selection_down() {
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
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert_eq!(state.status.mcp.mcp_panel_selection, 1);
}

#[test]
fn mcp_panel_ctrl_p_moves_selection_up() {
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

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert_eq!(state.status.mcp.mcp_panel_selection, 0);
}

#[test]
fn mcp_panel_j_is_noop() {
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
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);

    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );

    // j is no longer a navigation key in the MCPs panel — selection must not change
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);
}
