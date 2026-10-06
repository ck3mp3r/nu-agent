use super::*;

#[test]
fn model_picker_query_accepts_j_and_k_characters() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-a".to_string(),
                model: "jk-model-a".to_string(),
                identity: "jk-provider-a/jk-model-a".to_string(),
                display: "jk-provider-a / jk-model-a".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-b".to_string(),
                model: "jk-model-b".to_string(),
                identity: "jk-provider-b/jk-model-b".to_string(),
                display: "jk-provider-b / jk-model-b".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    let j_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    let k_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );

    assert!(j_changed);
    assert!(k_changed);
    assert_eq!(state.picker.active_state().unwrap().query, "jk");
    assert_eq!(state.picker.active_state().unwrap().filtered().len(), 2);
}

#[test]
fn model_picker_navigation_does_not_consume_query_jk_input() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-a".to_string(),
                model: "jk-model-a".to_string(),
                identity: "jk-provider-a/jk-model-a".to_string(),
                display: "jk-provider-a / jk-model-a".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-b".to_string(),
                model: "jk-model-b".to_string(),
                identity: "jk-provider-b/jk-model-b".to_string(),
                display: "jk-provider-b / jk-model-b".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-c".to_string(),
                model: "jk-model-c".to_string(),
                identity: "jk-provider-c/jk-model-c".to_string(),
                display: "jk-provider-c / jk-model-c".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    let down_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert!(down_changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let j_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(j_changed);
    assert_eq!(state.picker.active_state().unwrap().query, "j");
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    let k_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(k_changed);
    assert_eq!(state.picker.active_state().unwrap().query, "jk");
    assert_eq!(state.picker.active_state().unwrap().selection, 0);

    let down_again_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert!(down_again_changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let up_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Up), None);
    assert!(up_changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
}

#[test]
fn model_picker_ctrl_n_moves_to_next_item() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai / gpt-4o-mini".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic / claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);
}

#[test]
fn query_picker_ctrl_n_moves_to_next_item_consistently() {
    let mut palette_state = AppState::default();
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlP),
        None,
    );
    assert_eq!(
        palette_state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    let palette_changed = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlN),
        None,
    );
    assert!(palette_changed);
    assert_eq!(palette_state.picker.active_state().unwrap().selection, 1);

    let mut model_state = AppState::default();
    model_state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai / gpt-4o-mini".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic / claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    model_state.picker.open(ActivePicker::Model);

    let model_changed = dispatch_terminal_event(
        &mut model_state,
        &TerminalEvent::Key(TerminalKey::CtrlN),
        None,
    );
    assert!(model_changed);
    assert_eq!(model_state.picker.active_state().unwrap().selection, 1);
}

#[test]
fn model_picker_ctrl_p_moves_selection_up() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai / gpt-4o-mini".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic / claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    // Move down first
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(state.picker.active_state().unwrap().selection, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(state.picker.active_state().unwrap().selection, 0);
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
}
