use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The selection of the open picker, or the ok_or message when none is open.
fn picker_selection(state: &AppState) -> Result<usize> {
    Ok(state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .selection)
}

/// The query of the open picker, or the ok_or message when none is open.
fn picker_query(state: &AppState) -> Result<String> {
    Ok(state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .query
        .clone())
}

/// The number of rows the open picker's query keeps.
fn picker_filtered_len(state: &AppState) -> Result<usize> {
    Ok(state
        .picker
        .active_state()
        .ok_or("picker should be open")?
        .filtered()
        .len())
}

/// The selection of an open query picker in this state.
fn palette_selection(state: &AppState) -> Result<usize> {
    picker_selection(state)
}

#[test]
fn model_picker_navigation_table() -> Result<()> {
    // Model picker Ctrl-N moves to the next item.
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai / gpt-4o-mini".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
            },
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic / claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlN), None);

    assert!(changed);
    assert_eq!(picker_selection(&state)?, 1);

    // Query picker Ctrl-N moves to the next item consistently.
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
    assert_eq!(palette_selection(&palette_state)?, 1);

    // Model picker Ctrl-P moves the selection up.
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai / gpt-4o-mini".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
            },
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic / claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert_eq!(picker_selection(&state)?, 1);

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);

    assert!(changed);
    assert_eq!(picker_selection(&state)?, 0);
    assert_eq!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    Ok(())
}

#[test]
fn model_picker_query_table() -> Result<()> {
    // The query accepts j and k characters.
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-a".to_string(),
                model: "jk-model-a".to_string(),
                identity: "jk-provider-a/jk-model-a".to_string(),
                display: "jk-provider-a / jk-model-a".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
            },
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-b".to_string(),
                model: "jk-model-b".to_string(),
                identity: "jk-provider-b/jk-model-b".to_string(),
                display: "jk-provider-b / jk-model-b".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
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
    assert_eq!(picker_query(&state)?, "jk");
    assert_eq!(picker_filtered_len(&state)?, 2);

    // Navigation does not consume query jk input.
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-a".to_string(),
                model: "jk-model-a".to_string(),
                identity: "jk-provider-a/jk-model-a".to_string(),
                display: "jk-provider-a / jk-model-a".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
            },
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-b".to_string(),
                model: "jk-model-b".to_string(),
                identity: "jk-provider-b/jk-model-b".to_string(),
                display: "jk-provider-b / jk-model-b".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
            },
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "jk-provider-c".to_string(),
                model: "jk-model-c".to_string(),
                identity: "jk-provider-c/jk-model-c".to_string(),
                display: "jk-provider-c / jk-model-c".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
            },
        ],
    );
    state.picker.open(ActivePicker::Model);

    let down_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert!(down_changed);
    assert_eq!(picker_selection(&state)?, 1);

    let j_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(j_changed);
    assert_eq!(picker_query(&state)?, "j");
    assert_eq!(picker_selection(&state)?, 0);

    let k_changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(k_changed);
    assert_eq!(picker_query(&state)?, "jk");
    assert_eq!(picker_selection(&state)?, 0);

    let down_again_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    assert!(down_again_changed);
    assert_eq!(picker_selection(&state)?, 1);

    let up_changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Up), None);
    assert!(up_changed);
    assert_eq!(picker_selection(&state)?, 0);
    Ok(())
}
