use crate::{
    interaction::{
        cancel::CancelController,
        input::{TerminalEvent, map_terminal_event},
        reducer::{ReducerInput, UserAction, reduce_with_cancel_controller},
    },
    state::{AppState, InputMode, UiPhase},
};

pub fn dispatch_terminal_event(
    state: &mut AppState,
    event: &TerminalEvent,
    cancel_controller: Option<&CancelController>,
) -> bool {
    let Some(mapped_action) = map_terminal_event(event, state.input_locked) else {
        return false;
    };

    let (action, force_changed) = rewrite_action(state, mapped_action);
    let changed =
        reduce_with_cancel_controller(state, ReducerInput::User(action), cancel_controller);
    force_changed || changed
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
                        state.mcp_panel_move_up();
                        UserAction::Noop
                    }
                    UserAction::ScrollLineDown
                    | UserAction::HistoryDown
                    | UserAction::QueryNext => {
                        state.mcp_panel_move_down();
                        UserAction::Noop
                    }
                    UserAction::Submit | UserAction::InsertChar(' ') => {
                        let _ = state.queue_selected_mcp_toggle_request();
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

    if state.command_palette_open {
        return (
            match action {
                UserAction::ToggleCommandPalette => UserAction::CommandPaletteMoveUp,
                UserAction::Esc => UserAction::CommandPaletteClose,
                UserAction::EnterInsertMode => UserAction::Noop,
                UserAction::Submit => UserAction::CommandPaletteSelect,
                UserAction::ScrollLineUp | UserAction::HistoryUp => {
                    UserAction::CommandPaletteMoveUp
                }
                UserAction::ScrollLineDown | UserAction::HistoryDown => {
                    UserAction::CommandPaletteMoveDown
                }
                UserAction::QueryNext => UserAction::CommandPaletteMoveDown,
                UserAction::Backspace => {
                    state.backspace_command_palette_query_char();
                    UserAction::Noop
                }
                UserAction::InsertChar(ch) => {
                    state.append_command_palette_query_char(ch);
                    UserAction::Noop
                }
                UserAction::Quit => UserAction::Quit,
                _ => UserAction::Noop,
            },
            true,
        );
    }

    if state.model_picker_open {
        return (
            match action {
                UserAction::Esc => {
                    state.model_picker_close_on_escape();
                    UserAction::Noop
                }
                UserAction::Submit => {
                    let _ = state.queue_selected_model_switch_request();
                    state.close_model_picker();
                    UserAction::Noop
                }
                UserAction::ScrollLineUp | UserAction::HistoryUp => {
                    state.model_picker_move_up();
                    UserAction::Noop
                }
                UserAction::ScrollLineDown | UserAction::HistoryDown => {
                    state.model_picker_move_down();
                    UserAction::Noop
                }
                UserAction::QueryNext => {
                    state.model_picker_move_down();
                    UserAction::Noop
                }
                UserAction::ToggleCommandPalette => {
                    state.model_picker_move_up();
                    UserAction::Noop
                }
                UserAction::Backspace => {
                    state.backspace_model_picker_query_char();
                    UserAction::Noop
                }
                UserAction::InsertChar(ch) => {
                    state.append_model_picker_query_char(ch);
                    UserAction::Noop
                }
                _ => UserAction::Noop,
            },
            true,
        );
    }

    if state.agent_picker_open {
        return (
            match action {
                UserAction::Esc => {
                    state.agent_picker_close_on_escape();
                    UserAction::Noop
                }
                UserAction::Submit => {
                    let _ = state.queue_selected_agent_switch_request();
                    state.close_agent_picker();
                    UserAction::Noop
                }
                UserAction::ScrollLineUp | UserAction::HistoryUp => {
                    state.agent_picker_move_up();
                    UserAction::Noop
                }
                UserAction::ScrollLineDown | UserAction::HistoryDown => {
                    state.agent_picker_move_down();
                    UserAction::Noop
                }
                UserAction::QueryNext => {
                    state.agent_picker_move_down();
                    UserAction::Noop
                }
                UserAction::ToggleCommandPalette => {
                    state.agent_picker_move_up();
                    UserAction::Noop
                }
                UserAction::Backspace => {
                    state.backspace_agent_picker_query_char();
                    UserAction::Noop
                }
                UserAction::InsertChar(ch) => {
                    state.append_agent_picker_query_char(ch);
                    UserAction::Noop
                }
                _ => UserAction::Noop,
            },
            true,
        );
    }

    if state.theme_picker_open {
        return (
            match action {
                UserAction::Esc => {
                    state.theme_picker_close_on_escape();
                    UserAction::Noop
                }
                UserAction::Submit => {
                    let _ = state.queue_selected_theme_switch_request();
                    state.close_theme_picker();
                    UserAction::Noop
                }
                UserAction::ScrollLineUp | UserAction::HistoryUp => {
                    state.theme_picker_move_up();
                    UserAction::Noop
                }
                UserAction::ScrollLineDown | UserAction::HistoryDown => {
                    state.theme_picker_move_down();
                    UserAction::Noop
                }
                UserAction::QueryNext => {
                    state.theme_picker_move_down();
                    UserAction::Noop
                }
                UserAction::ToggleCommandPalette => {
                    state.theme_picker_move_up();
                    UserAction::Noop
                }
                _ => UserAction::Noop,
            },
            true,
        );
    }

    if state.session_picker_open {
        return (
            match action {
                UserAction::Esc => {
                    state.session_picker_close_on_escape();
                    UserAction::Noop
                }
                UserAction::Submit => {
                    if let Some(option) = state.selected_session_picker_option() {
                        state.queue_session_switch_request(option.id.clone());
                    }
                    state.close_session_picker();
                    UserAction::Noop
                }
                UserAction::ScrollLineUp | UserAction::HistoryUp => {
                    state.session_picker_move_up();
                    UserAction::Noop
                }
                UserAction::ScrollLineDown | UserAction::HistoryDown => {
                    state.session_picker_move_down();
                    UserAction::Noop
                }
                UserAction::QueryNext => {
                    state.session_picker_move_down();
                    UserAction::Noop
                }
                UserAction::ToggleCommandPalette => {
                    state.session_picker_move_up();
                    UserAction::Noop
                }
                UserAction::Backspace => {
                    state.backspace_session_picker_query_char();
                    UserAction::Noop
                }
                UserAction::InsertChar(ch) => {
                    state.append_session_picker_query_char(ch);
                    UserAction::Noop
                }
                _ => UserAction::Noop,
            },
            true,
        );
    }

    if state.has_permission_prompt() {
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

    if state.inline_slash_open && state.input_mode == InputMode::Insert {
        return (
            match action {
                UserAction::HistoryUp => UserAction::InlineSlashMoveUp,
                UserAction::HistoryDown => UserAction::InlineSlashMoveDown,
                UserAction::Submit | UserAction::CompleteForward => UserAction::InlineSlashAccept,
                UserAction::Esc => UserAction::InlineSlashClose,
                other => other,
            },
            false,
        );
    }

    // Tab cycles agents in insert mode
    if matches!(action, UserAction::CompleteForward)
        && state.input_mode == InputMode::Insert
        && !state.agent_picker_open
        && !state.model_picker_open
        && !state.session_picker_open
        && state.has_agents_to_cycle()
    {
        state.queue_cycle_agent_request();
        return (UserAction::Noop, true);
    }

    if state.phase == UiPhase::Idle {
        match state.input_mode {
            InputMode::Normal => {
                state.clear_insert_exit_pending_j();
                return (
                    match action {
                        UserAction::ToggleCommandPalette => {
                            state.clear_normal_pending_key();
                            UserAction::ToggleCommandPalette
                        }
                        UserAction::InsertChar('i') => {
                            state.clear_normal_pending_key();
                            UserAction::EnterInsertMode
                        }
                        UserAction::InsertChar('v') => {
                            state.clear_normal_pending_key();
                            UserAction::EnterVisualMode
                        }
                        UserAction::InsertChar('j') => {
                            state.clear_normal_pending_key();
                            UserAction::ScrollLineDown
                        }
                        UserAction::InsertChar('k') => {
                            state.clear_normal_pending_key();
                            UserAction::ScrollLineUp
                        }
                        UserAction::InsertChar('h') => {
                            state.clear_normal_pending_key();
                            UserAction::FocusPaneLeft
                        }
                        UserAction::InsertChar('l') => {
                            state.clear_normal_pending_key();
                            UserAction::FocusPaneRight
                        }
                        UserAction::CompleteForward => {
                            state.clear_normal_pending_key();
                            UserAction::FocusPaneRight
                        }
                        UserAction::CompleteBackward => {
                            state.clear_normal_pending_key();
                            UserAction::FocusPaneLeft
                        }
                        UserAction::InsertChar('g') => {
                            if state.take_normal_pending_key_if('g') {
                                UserAction::ScrollToTop
                            } else {
                                state.arm_normal_pending_key('g');
                                UserAction::Noop
                            }
                        }
                        UserAction::InsertChar('G') => {
                            state.clear_normal_pending_key();
                            UserAction::ScrollToBottom
                        }
                        UserAction::ScrollPageUp | UserAction::ScrollPageDown | UserAction::Esc => {
                            state.clear_normal_pending_key();
                            action
                        }
                        UserAction::InsertChar(_) => {
                            state.clear_normal_pending_key();
                            UserAction::Noop
                        }
                        UserAction::InsertNewline => {
                            state.clear_normal_pending_key();
                            UserAction::Noop
                        }
                        other => {
                            state.clear_normal_pending_key();
                            other
                        }
                    },
                    false,
                );
            }
            InputMode::Visual => {
                return (
                    match action {
                        UserAction::ToggleCommandPalette => {
                            state.clear_normal_pending_key();
                            UserAction::ToggleCommandPalette
                        }
                        UserAction::InsertChar('j') => {
                            state.clear_normal_pending_key();
                            UserAction::ScrollLineDown
                        }
                        UserAction::InsertChar('k') => {
                            state.clear_normal_pending_key();
                            UserAction::ScrollLineUp
                        }
                        UserAction::InsertChar('g') => {
                            if state.take_normal_pending_key_if('g') {
                                UserAction::ScrollToTop
                            } else {
                                state.arm_normal_pending_key('g');
                                UserAction::Noop
                            }
                        }
                        UserAction::InsertChar('G') => {
                            state.clear_normal_pending_key();
                            UserAction::ScrollToBottom
                        }
                        UserAction::InsertChar('y') => {
                            state.clear_normal_pending_key();
                            UserAction::YankSelection
                        }
                        UserAction::Esc => {
                            state.clear_normal_pending_key();
                            UserAction::Esc
                        }
                        UserAction::ScrollPageUp | UserAction::ScrollPageDown => {
                            state.clear_normal_pending_key();
                            action
                        }
                        UserAction::InsertNewline => {
                            state.clear_normal_pending_key();
                            UserAction::Noop
                        }
                        _ => {
                            state.clear_normal_pending_key();
                            UserAction::Noop
                        }
                    },
                    false,
                );
            }
            InputMode::Insert => {
                return (
                    match action {
                        UserAction::ToggleCommandPalette => {
                            state.clear_insert_exit_pending_j();
                            state.clear_normal_pending_key();
                            UserAction::ToggleCommandPalette
                        }
                        UserAction::InsertChar('j') => {
                            if state.insert_exit_pending_j() {
                                state.clear_insert_exit_pending_j();
                                UserAction::EnterNormalModeFromChord
                            } else {
                                state.set_insert_exit_pending_j();
                                UserAction::InsertChar('j')
                            }
                        }
                        UserAction::InsertChar('k') => {
                            if state.insert_exit_pending_j() {
                                state.clear_insert_exit_pending_j();
                                UserAction::EnterNormalModeFromChord
                            } else {
                                state.clear_insert_exit_pending_j();
                                UserAction::InsertChar('k')
                            }
                        }
                        UserAction::Esc => {
                            state.clear_insert_exit_pending_j();
                            state.clear_normal_pending_key();
                            UserAction::Esc
                        }
                        other => {
                            state.clear_insert_exit_pending_j();
                            state.clear_normal_pending_key();
                            other
                        }
                    },
                    false,
                );
            }
        }
    }

    if state.input_mode == InputMode::Insert {
        return match action {
            UserAction::InsertChar('j') => {
                if state.insert_exit_pending_j() {
                    state.clear_insert_exit_pending_j();
                    state.enter_normal_mode();
                    (UserAction::EnterNormalModeFromChord, true)
                } else {
                    state.set_insert_exit_pending_j();
                    (UserAction::InsertChar('j'), true) // force_changed: j/k chord tracking modifies state
                }
            }
            UserAction::InsertChar('k') => {
                if state.insert_exit_pending_j() {
                    state.clear_insert_exit_pending_j();
                    state.enter_normal_mode();
                    (UserAction::EnterNormalModeFromChord, true)
                } else {
                    state.clear_insert_exit_pending_j();
                    (UserAction::InsertChar('k'), true) // force_changed: j/k chord tracking modifies state
                }
            }
            UserAction::Esc => {
                state.clear_insert_exit_pending_j();
                state.clear_normal_pending_key();
                state.enter_normal_mode();
                state.set_insert_exit_pending_j();
                (UserAction::Noop, true)
            }
            other => {
                state.clear_insert_exit_pending_j();
                state.clear_normal_pending_key();
                (other, false)
            }
        };
    }

    if state.input_mode == InputMode::Normal {
        let (rewritten, force_changed) = match action {
            UserAction::InsertChar('i') => {
                state.clear_normal_pending_key();
                state.enter_insert_mode();
                (UserAction::Noop, true)
            }
            UserAction::InsertChar('j') => {
                state.clear_normal_pending_key();
                (UserAction::ScrollLineDown, false)
            }
            UserAction::InsertChar('k') => {
                state.clear_normal_pending_key();
                (UserAction::ScrollLineUp, false)
            }
            UserAction::InsertChar('h') => {
                state.clear_normal_pending_key();
                (UserAction::FocusPaneLeft, false)
            }
            UserAction::InsertChar('l') => {
                state.clear_normal_pending_key();
                (UserAction::FocusPaneRight, false)
            }
            UserAction::CompleteForward => {
                state.clear_normal_pending_key();
                (UserAction::FocusPaneRight, false)
            }
            UserAction::CompleteBackward => {
                state.clear_normal_pending_key();
                (UserAction::FocusPaneLeft, false)
            }
            UserAction::InsertChar('g') => {
                if state.take_normal_pending_key_if('g') {
                    (UserAction::ScrollToTop, false)
                } else {
                    state.arm_normal_pending_key('g');
                    (UserAction::Noop, false)
                }
            }
            UserAction::InsertChar('G') => {
                state.clear_insert_exit_pending_j();
                state.clear_normal_pending_key();
                (UserAction::ScrollToBottom, false)
            }
            UserAction::ScrollPageUp | UserAction::ScrollPageDown => {
                state.clear_insert_exit_pending_j();
                state.clear_normal_pending_key();
                (action, false)
            }
            UserAction::Esc => {
                let fast_confirm_after_mode_switch = state.insert_exit_pending_j();
                state.clear_normal_pending_key();

                if state.phase == UiPhase::AbortPending && state.abort.pending {
                    state.clear_insert_exit_pending_j();
                    (UserAction::EscConfirm, false)
                } else if state.phase == UiPhase::Busy && fast_confirm_after_mode_switch {
                    state.clear_insert_exit_pending_j();
                    state.enter_insert_mode();
                    state.request_abort_confirmation();
                    (UserAction::EscConfirm, true)
                } else {
                    state.clear_insert_exit_pending_j();
                    (UserAction::Esc, false)
                }
            }
            UserAction::InsertChar(_) | UserAction::InsertNewline => {
                state.clear_insert_exit_pending_j();
                state.clear_normal_pending_key();
                (UserAction::Noop, false)
            }
            other => {
                state.clear_insert_exit_pending_j();
                state.clear_normal_pending_key();
                (other, false)
            }
        };

        return (rewritten, force_changed);
    }

    let fast_confirm_after_mode_switch = state.insert_exit_pending_j();
    if !matches!(action, UserAction::Esc) {
        state.clear_insert_exit_pending_j();
    }
    state.clear_normal_pending_key();

    (
        match action {
            UserAction::Esc => {
                if state.phase == UiPhase::AbortPending && state.abort.pending {
                    state.clear_insert_exit_pending_j();
                    UserAction::EscConfirm
                } else if state.phase == UiPhase::Busy && fast_confirm_after_mode_switch {
                    state.clear_insert_exit_pending_j();
                    state.enter_insert_mode();
                    state.request_abort_confirmation();
                    UserAction::EscConfirm
                } else {
                    state.clear_insert_exit_pending_j();
                    UserAction::Esc
                }
            }
            _ => action,
        },
        false,
    )
}
