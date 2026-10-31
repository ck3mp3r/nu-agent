use super::*;

#[test]
fn mcps_panel_mutations_mark_render() {
    use crate::interaction::dispatch::{KeyLens, rewrite_action};
    use agnt_core::protocol::events::Key;

    // Start the selection away from the wrap target so every move changes the
    // row the panel highlights.
    let cases = [
        (Key::Up, 1usize),
        (Key::CtrlP, 1),
        (Key::CtrlK, 1),
        (Key::Down, 0),
        (Key::CtrlN, 0),
        (Key::CtrlJ, 0),
        (Key::Enter, 0),
        (Key::Char(' '), 0),
    ];
    for (key, start) in cases {
        // -- Setup & Fixtures
        let mut state = two_server_state();
        state.status.mcp.mcp_panel_selection = start;

        // -- Exec
        let lens = rewrite_action(&mut state, key);

        // -- Check: the panel owns the key and the frame is marked dirty.
        assert!(
            matches!(lens, KeyLens::Consumed(true)),
            "the panel owns {key:?} and the mutation marks the frame dirty"
        );
    }
}

#[test]
fn mcps_panel_noop_mutations_keep_the_frame_still() {
    use crate::interaction::dispatch::{KeyLens, rewrite_action};
    use agnt_core::protocol::events::Key;

    // -- Check: one server wraps a move onto the same row, which draws nothing new.
    for key in [Key::Up, Key::Down, Key::CtrlP, Key::CtrlN] {
        let mut state = AppState::default();
        state.status.mcp.set_mcp_servers(vec![McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
        state.open_info_panel(InfoPanel::Mcps);

        let lens = rewrite_action(&mut state, key);
        assert!(
            matches!(lens, KeyLens::Consumed(false)),
            "a same-row move must not force a render: {key:?}"
        );
    }

    // -- Check: no server means no toggle request and nothing to redraw.
    let mut empty = AppState::default();
    empty.open_info_panel(InfoPanel::Mcps);
    let lens = rewrite_action(&mut empty, Key::Enter);
    assert!(
        matches!(lens, KeyLens::Consumed(false)),
        "a toggle with no server must not force a render"
    );

    // -- Check: a printable key filters instead of being swallowed.
    let mut typing = two_server_state();
    let lens = rewrite_action(&mut typing, Key::Char('j'));
    assert!(
        matches!(lens, KeyLens::Consumed(true)),
        "a typed character must re-filter and repaint"
    );
    assert_eq!(typing.status.mcp.mcp_panel_query(), "j");

    // -- Check: Backspace with an empty query changes nothing.
    let mut bare = two_server_state();
    let lens = rewrite_action(&mut bare, Key::Backspace);
    assert!(
        matches!(lens, KeyLens::Consumed(false)),
        "an empty query has nothing to erase"
    );

    // -- Check: Esc still passes through to the reducer that closes the panel.
    let mut esc = two_server_state();
    let lens = rewrite_action(&mut esc, Key::Esc);
    assert!(
        matches!(lens, KeyLens::Escape),
        "Esc is the reducer's key, not the panel's"
    );
}

/// Type-to-filter through the real dispatcher: printable characters build the
/// query and re-filter, Space toggles instead of filtering, Backspace erases,
/// and Esc closes while discarding the query.
#[test]
fn mcps_panel_type_to_filter_through_the_dispatcher() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = three_server_state();
    state.open_info_panel(InfoPanel::Mcps);

    // -- Exec: type "c7".
    for ch in "c7".chars() {
        let changed =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char(ch)), None);
        assert!(changed, "typing must repaint: {ch}");
    }

    // -- Check: the query is held and the rows narrowed to the subsequence match.
    assert_eq!(state.status.mcp.mcp_panel_query(), "c7");
    let rows = state.status.mcp.mcp_servers_filtered();
    let names = rows.iter().map(|s| s.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, vec!["context7"]);
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);

    // -- Exec: Space toggles the selected row of the filtered view.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char(' ')),
        None,
    );

    // -- Check: Space did not enter the query, and the filtered row was toggled.
    assert!(changed);
    assert_eq!(state.status.mcp.mcp_panel_query(), "c7");
    assert_eq!(
        state.status.mcp.selected_mcp_server_name(),
        Some("context7")
    );
    let request = state
        .status
        .mcp
        .take_next_mcp_toggle_request()
        .ok_or("should have toggle request")?;
    assert_eq!(request.server_name, "context7");
    assert!(request.enable);

    // -- Exec & Check: Backspace erases the last query character.
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Backspace),
        None,
    );
    assert_eq!(state.status.mcp.mcp_panel_query(), "c");

    // -- Exec & Check: Esc closes the panel and discards the query.
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert_eq!(state.info_panel, None);
    assert_eq!(state.status.mcp.mcp_panel_query(), "");
    Ok(())
}

/// PgUp/PgDn and Ctrl-U/Ctrl-D page the MCPs selection 8 rows, clamped to the
/// filtered list's bounds, while arrow moves stay one row.
#[test]
fn mcps_panel_page_moves_clamp() -> Result<()> {
    use crate::interaction::dispatch::{KeyLens, rewrite_action};
    use agnt_core::protocol::events::Key;

    // -- Setup & Fixtures: twenty servers.
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(
        (0..20)
            .map(|idx| McpServerState {
                name: format!("srv-{idx:02}"),
                state: McpServerUsabilityState::Enabled,
            })
            .collect(),
    );
    state.open_info_panel(InfoPanel::Mcps);

    // -- Exec & Check: a page down lands eight rows lower.
    let lens = rewrite_action(&mut state, Key::PageDown);
    assert!(matches!(lens, KeyLens::Consumed(true)));
    assert_eq!(state.status.mcp.mcp_panel_selection, 8);

    // -- Exec & Check: the last page clamps to the last row.
    rewrite_action(&mut state, Key::PageDown);
    assert_eq!(state.status.mcp.mcp_panel_selection, 16);
    rewrite_action(&mut state, Key::PageDown);
    assert_eq!(state.status.mcp.mcp_panel_selection, 19);
    let lens = rewrite_action(&mut state, Key::PageDown);
    assert_eq!(state.status.mcp.mcp_panel_selection, 19);
    assert!(
        matches!(lens, KeyLens::Consumed(false)),
        "a clamped page changes nothing"
    );

    // -- Exec & Check: pages walk back up and clamp at the first row.
    rewrite_action(&mut state, Key::PageUp);
    assert_eq!(state.status.mcp.mcp_panel_selection, 11);
    rewrite_action(&mut state, Key::PageUp);
    assert_eq!(state.status.mcp.mcp_panel_selection, 3);
    rewrite_action(&mut state, Key::PageUp);
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);
    rewrite_action(&mut state, Key::PageUp);
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);

    // -- Exec & Check: arrow moves still move exactly one row.
    rewrite_action(&mut state, Key::Down);
    assert_eq!(state.status.mcp.mcp_panel_selection, 1);

    // -- Exec & Check: the terminal keys reach the same page move.
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::PageDown), None);
    assert_eq!(state.status.mcp.mcp_panel_selection, 9);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlD), None);
    assert_eq!(state.status.mcp.mcp_panel_selection, 17);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlU), None);
    assert_eq!(state.status.mcp.mcp_panel_selection, 9);
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::PageUp), None);
    assert_eq!(state.status.mcp.mcp_panel_selection, 1);

    // -- Exec: a query narrowing the list to ten rows.
    for ch in "srv-0".chars() {
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Char(ch)), None);
    }

    // -- Check: the filtered view keeps its own bounds. The subsequence keeps
    // srv-00..srv-10 (`0` still appears after the dash in srv-10).
    let len = state.status.mcp.mcp_servers_filtered().len();
    assert_eq!(len, 11);
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);
    rewrite_action(&mut state, Key::PageDown);
    assert_eq!(state.status.mcp.mcp_panel_selection, 8);
    rewrite_action(&mut state, Key::PageDown);
    assert_eq!(state.status.mcp.mcp_panel_selection, len - 1);
    rewrite_action(&mut state, Key::PageUp);
    assert_eq!(state.status.mcp.mcp_panel_selection, len - 1 - 8);
    Ok(())
}

/// An open MCPs panel holding two servers.
fn two_server_state() -> AppState {
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
    state
}

/// An open MCPs panel holding three servers.
fn three_server_state() -> AppState {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        McpServerState {
            name: "c5t".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        McpServerState {
            name: "context7".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        McpServerState {
            name: "github".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
    ]);
    state.open_info_panel(InfoPanel::Mcps);
    state
}

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
fn mcp_panel_control_j_and_k_move_without_touching_the_filter() {
    // -- Setup & Fixtures
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

    // -- Exec: Ctrl-J moves the selection down one row.
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlJ), None);

    // -- Check: the move repaints and leaves the filter empty.
    assert!(changed);
    assert_eq!(state.status.mcp.mcp_panel_selection, 1);
    assert_eq!(state.status.mcp.mcp_panel_query(), "");

    // -- Exec: Ctrl-K moves it back up.
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlK), None);

    // -- Check
    assert!(changed);
    assert_eq!(state.status.mcp.mcp_panel_selection, 0);
    assert_eq!(state.status.mcp.mcp_panel_query(), "");
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
