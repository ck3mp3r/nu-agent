use super::*;

#[test]
fn set_picker_options_sorts_agent_by_id() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![
            AgentPickerOption {
                name: "zeta".into(),
                description: None,
                display: "zeta".into(),
                builtin: false,
            },
            AgentPickerOption {
                name: "alpha".into(),
                description: None,
                display: "alpha".into(),
                builtin: false,
            },
        ],
    );

    let ids: Vec<String> = state.picker.entries[2]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["alpha", "zeta"]);
}

#[test]
fn set_picker_options_sorts_session_by_created_at_desc() {
    let mut state = AppState::default();
    let now = chrono::Utc::now();
    state.set_picker_options(
        ActivePicker::Session,
        vec![
            PickerOption {
                id: "old".to_string(),
                display: "old".to_string(),
                search_text: "old".to_string(),
                sort_key: vec![PickerSortKeyPart::Recent(std::cmp::Reverse(
                    now - chrono::Duration::days(1),
                ))],
                payload: PickerPayload::Session {
                    session_id: "old".to_string(),
                    title: None,
                    created_at: now - chrono::Duration::days(1),
                    message_count: 1,
                },
            },
            PickerOption {
                id: "new".to_string(),
                display: "new".to_string(),
                search_text: "new".to_string(),
                sort_key: vec![PickerSortKeyPart::Recent(std::cmp::Reverse(now))],
                payload: PickerPayload::Session {
                    session_id: "new".to_string(),
                    title: None,
                    created_at: now,
                    message_count: 1,
                },
            },
        ],
    );

    let ids: Vec<String> = state.picker.entries[3]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["new", "old"]);
}

#[test]
fn set_picker_options_keeps_theme_unsorted() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Theme,
        vec![
            PickerOption {
                id: "b".to_string(),
                display: "b".to_string(),
                search_text: "b".to_string(),
                sort_key: Vec::new(),
                payload: PickerPayload::Theme,
            },
            PickerOption {
                id: "a".to_string(),
                display: "a".to_string(),
                search_text: "a".to_string(),
                sort_key: Vec::new(),
                payload: PickerPayload::Theme,
            },
        ],
    );

    let ids: Vec<String> = state.picker.entries[4]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["b", "a"]);
}

#[test]
fn set_picker_options_writes_without_opening() {
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::Model, Vec::<PickerOption>::new());

    assert_eq!(state.picker.render_kind(), None);
    assert!(state.picker.entries[1].state.options.is_empty());
}

#[test]
fn set_picker_options_sorts_model_by_provider_then_identity() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            ModelPickerOption {
                provider: "z-provider".to_string(),
                model: "z-model".to_string(),
                identity: "z-provider/z-model".to_string(),
                display: "z".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            ModelPickerOption {
                provider: "a-provider".to_string(),
                model: "a-model".to_string(),
                identity: "a-provider/a-model".to_string(),
                display: "a".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );

    let ids: Vec<String> = state.picker.entries[1]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["a-provider/a-model", "z-provider/z-model"]);
}

#[test]
fn set_picker_options_sorts_model_configured_first() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai/gpt-4o-mini".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                identity: "openai/gpt-4o".to_string(),
                display: "openai/gpt-4o".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: true,
                provider_display_name: String::new(),
            },
        ],
    );

    let ids: Vec<String> = state.picker.entries[1]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["openai/gpt-4o", "openai/gpt-4o-mini"]);
}

#[test]
fn open_command_palette_twice_opens_single_picker() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);
    open_command_palette_for_test(&mut state);

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(state.picker.active(), Some(ActivePicker::CommandPalette));
}
