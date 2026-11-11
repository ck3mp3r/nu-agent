use super::*;

pub(super) fn open_permission_prompt(state: &mut AppState) {
    state.permission.open_prompt("ask-0000000000000001");
}

pub(super) fn busy_state_with_controller() -> (AppState, CancelController) {
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();
    state.input.pending_submit_text = Some("w".to_string());
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Enter),
        Some(&cancel_controller),
    );
    (state, cancel_controller)
}

pub(super) fn busy_state() -> AppState {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("w".to_string()),
        ..Default::default()
    };
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    state
}

pub(super) fn setup_agent_picker_open() -> AppState {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![
            agnt_runtime::protocol::picker::AgentPickerOption {
                name: "alpha".into(),
                description: Some("Alpha agent".into()),
                display: "alpha — Alpha agent".into(),
                builtin: false,
            },
            agnt_runtime::protocol::picker::AgentPickerOption {
                name: "beta".into(),
                description: None,
                display: "beta".into(),
                builtin: false,
            },
            agnt_runtime::protocol::picker::AgentPickerOption {
                name: "gamma".into(),
                description: Some("Gamma agent".into()),
                display: "gamma — Gamma agent".into(),
                builtin: false,
            },
        ],
    );
    state.picker.open(ActivePicker::Agent);
    state
}
