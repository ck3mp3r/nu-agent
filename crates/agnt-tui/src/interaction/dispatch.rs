use crate::{
    interaction::{cancel::CancelController, input::TerminalEvent, reducer::UserAction},
    state::{AppState, InputMode, PANEL_PAGE_LINES, PaneFocus, UiPhase},
};

pub fn dispatch_terminal_event(
    state: &mut AppState,
    event: &TerminalEvent,
    cancel_controller: Option<&CancelController>,
) -> bool {
    state.reduce_terminal_event(event, cancel_controller)
}

pub(crate) fn rewrite_action(state: &mut AppState, action: UserAction) -> (UserAction, bool) {
    if let Some(panel) = state.info_panel {
        return (
            match panel {
                crate::state::InfoPanel::Mcps => match action {
                    UserAction::Esc => {
                        state.status.mcp.clear_mcp_panel_query();
                        UserAction::Esc
                    }
                    UserAction::Quit => UserAction::Quit,
                    // The frame repaints only while the dirty flag is set, so
                    // each mutation reports whether it changed what the panel
                    // draws. A move that wraps onto the same row changes nothing.
                    UserAction::ScrollLineUp
                    | UserAction::HistoryUp
                    | UserAction::ToggleCommandPalette
                    | UserAction::QueryPrev => {
                        let before = state.status.mcp.mcp_panel_selection;
                        state.status.mcp.mcp_panel_move_up();
                        let moved = state.status.mcp.mcp_panel_selection != before;
                        return (UserAction::Noop, moved);
                    }
                    UserAction::ScrollLineDown
                    | UserAction::HistoryDown
                    | UserAction::QueryNext => {
                        let before = state.status.mcp.mcp_panel_selection;
                        state.status.mcp.mcp_panel_move_down();
                        let moved = state.status.mcp.mcp_panel_selection != before;
                        return (UserAction::Noop, moved);
                    }
                    UserAction::ScrollPageUp => {
                        let before = state.status.mcp.mcp_panel_selection;
                        state.status.mcp.mcp_panel_page_up();
                        let moved = state.status.mcp.mcp_panel_selection != before;
                        return (UserAction::Noop, moved);
                    }
                    UserAction::ScrollPageDown => {
                        let before = state.status.mcp.mcp_panel_selection;
                        state.status.mcp.mcp_panel_page_down();
                        let moved = state.status.mcp.mcp_panel_selection != before;
                        return (UserAction::Noop, moved);
                    }
                    UserAction::Submit | UserAction::InsertChar(' ') => {
                        let queued = state.status.mcp.queue_selected_mcp_toggle_request();
                        return (UserAction::Noop, queued);
                    }
                    // Every other character filters the panel, as it does in the
                    // palette and the pickers. Space stays with the toggle.
                    UserAction::InsertChar(ch) => {
                        state.status.mcp.append_mcp_panel_query_char(ch);
                        return (UserAction::Noop, true);
                    }
                    UserAction::Backspace => {
                        let changed = state.status.mcp.backspace_mcp_panel_query_char();
                        return (UserAction::Noop, changed);
                    }
                    _ => UserAction::Noop,
                },
                _ => match action {
                    UserAction::Esc => UserAction::Esc,
                    UserAction::Quit => UserAction::Quit,
                    UserAction::ScrollLineUp
                    | UserAction::HistoryUp
                    | UserAction::ToggleCommandPalette
                    | UserAction::QueryPrev => {
                        state.info_panel_scroll = state.info_panel_scroll.saturating_sub(1);
                        return (UserAction::Noop, true);
                    }
                    UserAction::ScrollLineDown
                    | UserAction::HistoryDown
                    | UserAction::QueryNext => {
                        state.info_panel_scroll = state.info_panel_scroll.saturating_add(1);
                        return (UserAction::Noop, true);
                    }
                    UserAction::ScrollPageUp => {
                        state.info_panel_scroll =
                            state.info_panel_scroll.saturating_sub(PANEL_PAGE_LINES);
                        return (UserAction::Noop, true);
                    }
                    UserAction::ScrollPageDown => {
                        state.info_panel_scroll =
                            state.info_panel_scroll.saturating_add(PANEL_PAGE_LINES);
                        return (UserAction::Noop, true);
                    }
                    _ => UserAction::Noop,
                },
            },
            false,
        );
    }

    if state.picker.active().is_some() {
        return state.picker.handle_action(action);
    }

    // The card owns its decision keys, but not Esc: Esc falls through to the
    // busy-mode escalation and the input-mode rewriting below, so it exits
    // insert mode or arms the turn-cancel confirmation instead of denying.
    if state.permission.has_prompt() && !matches!(action, UserAction::Esc) {
        return (
            match action {
                UserAction::InsertChar('a') => UserAction::PermissionAllowOnce,
                UserAction::InsertChar('A') => UserAction::PermissionAllowAlways,
                UserAction::InsertChar('d') => UserAction::PermissionDeny,
                UserAction::HistoryUp => UserAction::ScrollLineUp,
                UserAction::HistoryDown => UserAction::ScrollLineDown,
                UserAction::ScrollLineUp
                | UserAction::ScrollLineDown
                | UserAction::ScrollPageUp
                | UserAction::ScrollPageDown
                | UserAction::ScrollToTop
                | UserAction::ScrollToBottom => action,
                _ => UserAction::Noop,
            },
            true,
        );
    }

    // Tab cycles agents in insert mode
    if matches!(action, UserAction::CompleteForward)
        && state.input.mode == InputMode::Insert
        && state.picker.active().is_none()
        && state.has_agents_to_cycle()
    {
        state.queue_cycle_agent_request();
        return (UserAction::Noop, true);
    }

    // Busy-mode Esc escalation: AbortPending escalates Esc to EscConfirm. This
    // couples the orchestrator phase/abort state (lifecycle-owned, not input
    // state) with the reducer, so it is resolved here; InputState handles all
    // remaining mode-specific rewriting below.
    if state.phase != UiPhase::Idle
        && matches!(action, UserAction::Esc)
        && state.input.mode != InputMode::Insert
    {
        state.input.clear_normal_pending_key();
        if state.phase == UiPhase::AbortPending && state.abort.pending {
            state.input.clear_insert_exit_pending_j();
            return (UserAction::EscConfirm, false);
        }
        state.input.clear_insert_exit_pending_j();
        return (UserAction::Esc, false);
    }

    // Input-mode rewriting: vim keymap, exit chords, pending keys. InputState
    // owns the mode; AppState owns pane focus, so the focus invariant is
    // re-synced here whenever handle_action transitions the mode.
    let mode_before = state.input.mode;
    let (rewritten, force_changed) = state
        .input
        .handle_action(action, state.phase == UiPhase::Idle);
    if state.input.mode != mode_before {
        state.scroll.pane_focus = match state.input.mode {
            InputMode::Insert => PaneFocus::Input,
            InputMode::Normal | InputMode::Visual => PaneFocus::Transcript,
        };
    }
    (rewritten, force_changed)
}
