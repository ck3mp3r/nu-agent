use crate::commands::agent::ui::tui::{
    cancel::CancelController,
    input::{TerminalEvent, map_terminal_event},
    reducer::{ReducerInput, UserAction, reduce_with_cancel_controller},
    state::{AppState, InputMode, UiPhase},
};

pub fn dispatch_terminal_event(
    state: &mut AppState,
    event: &TerminalEvent,
    cancel_controller: Option<&CancelController>,
) -> bool {
    let Some(mapped_action) = map_terminal_event(event, state.input.locked) else {
        return false;
    };

    let action = rewrite_action(state, mapped_action);
    let previous = state.clone();

    reduce_with_cancel_controller(state, ReducerInput::User(action), cancel_controller);

    *state != previous
}

fn rewrite_action(state: &mut AppState, action: UserAction) -> UserAction {
    if state.phase == UiPhase::Idle {
        match state.input_mode {
            InputMode::Normal => {
                state.set_insert_exit_pending_j(false);
                return match action {
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
                    UserAction::ScrollPageUp
                    | UserAction::ScrollPageDown
                    | UserAction::Esc => {
                        state.clear_normal_pending_key();
                        action
                    }
                    UserAction::InsertChar(_) => {
                        state.clear_normal_pending_key();
                        UserAction::Noop
                    }
                    other => {
                        state.clear_normal_pending_key();
                        other
                    }
                };
            }
            InputMode::Visual => {
                return match action {
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
                    _ => {
                        state.clear_normal_pending_key();
                        UserAction::Noop
                    }
                };
            }
            InputMode::Insert => {
                return match action {
                    UserAction::InsertChar('j') => {
                        if state.insert_exit_pending_j() {
                            state.set_insert_exit_pending_j(false);
                            UserAction::EnterNormalModeFromChord
                        } else {
                            state.set_insert_exit_pending_j(true);
                            UserAction::InsertChar('j')
                        }
                    }
                    UserAction::InsertChar('k') => {
                        if state.insert_exit_pending_j() {
                            state.set_insert_exit_pending_j(false);
                            UserAction::EnterNormalModeFromChord
                        } else {
                            state.set_insert_exit_pending_j(false);
                            UserAction::InsertChar('k')
                        }
                    }
                    UserAction::Esc => {
                        state.set_insert_exit_pending_j(false);
                        state.clear_normal_pending_key();
                        UserAction::Esc
                    }
                    other => {
                        state.set_insert_exit_pending_j(false);
                        state.clear_normal_pending_key();
                        other
                    }
                };
            }
        }
    }

    state.set_insert_exit_pending_j(false);
    state.clear_normal_pending_key();

    match action {
        UserAction::Esc => {
            if state.phase == UiPhase::AbortPending && state.abort.pending {
                UserAction::EscConfirm
            } else {
                UserAction::Esc
            }
        }
        _ => action,
    }
}
