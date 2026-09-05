use crate::{
    interaction::{cancel::CancelController, input::TerminalEvent, reducer::UserAction},
    state::{AppState, InputMode, PaneFocus, UiPhase},
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
                    UserAction::Esc => UserAction::Esc,
                    UserAction::Quit => UserAction::Quit,
                    UserAction::ScrollLineUp
                    | UserAction::HistoryUp
                    | UserAction::ToggleCommandPalette => {
                        state.status.mcp.mcp_panel_move_up();
                        UserAction::Noop
                    }
                    UserAction::ScrollLineDown
                    | UserAction::HistoryDown
                    | UserAction::QueryNext => {
                        state.status.mcp.mcp_panel_move_down();
                        UserAction::Noop
                    }
                    UserAction::Submit | UserAction::InsertChar(' ') => {
                        let _ = state.status.mcp.queue_selected_mcp_toggle_request();
                        UserAction::Noop
                    }
                    _ => UserAction::Noop,
                },
                _ => {
                    const PANEL_PAGE_LINES: usize = 8;
                    match action {
                        UserAction::Esc => UserAction::Esc,
                        UserAction::Quit => UserAction::Quit,
                        UserAction::ScrollLineUp
                        | UserAction::HistoryUp
                        | UserAction::ToggleCommandPalette => {
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
                    }
                }
            },
            false,
        );
    }

    if state.picker.active().is_some() {
        return state.picker.handle_action(action);
    }

    if state.permission.has_prompt() {
        return (
            match action {
                UserAction::InsertChar('a') => UserAction::PermissionAllowOnce,
                UserAction::InsertChar('A') => UserAction::PermissionAllowAlways,
                UserAction::InsertChar('d') => UserAction::PermissionDeny,
                UserAction::Esc => UserAction::PermissionDeny,
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

    // Busy-mode Esc escalation: the insert-exit chord arms a fast-confirm
    // abort, and AbortPending escalates Esc to EscConfirm. This couples input
    // chords with the orchestrator phase/abort state (lifecycle-owned, not
    // input state), so it is resolved here; InputState handles all remaining
    // mode-specific rewriting below.
    if state.phase != UiPhase::Idle
        && matches!(action, UserAction::Esc)
        && state.input.mode != InputMode::Insert
    {
        let fast_confirm_after_mode_switch = state.input.insert_exit_pending_j();
        state.input.clear_normal_pending_key();
        if state.phase == UiPhase::AbortPending && state.abort.pending {
            state.input.clear_insert_exit_pending_j();
            return (UserAction::EscConfirm, false);
        }
        if state.phase == UiPhase::Busy && fast_confirm_after_mode_switch {
            state.input.clear_insert_exit_pending_j();
            state.enter_insert_mode();
            state.request_abort_confirmation();
            return (UserAction::EscConfirm, true);
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
