use super::*;

/// `set_picker_options` sorts agent options by id, session options by created_at
/// descending, and model options by configured-first then provider then
/// identity; theme options keep their order. Writing options does not open the
/// picker, and opening the palette twice opens a single picker.
#[test]
fn set_picker_options_sorting_and_open() -> Result<()> {
    // -- Setup & Fixtures: agent options out of order.
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

    // -- Check: sorted by id.
    let ids: Vec<String> = state.picker.entries[2]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["alpha", "zeta"]);

    // -- Setup & Fixtures: session options out of order.
    let mut sessions = AppState::default();
    let now = chrono::Utc::now();
    sessions.set_picker_options(
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

    // -- Check: sorted by created_at descending.
    let ids: Vec<String> = sessions.picker.entries[3]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["new", "old"]);

    // -- Setup & Fixtures: theme options.
    let mut themes = AppState::default();
    themes.set_picker_options(
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

    // -- Check: theme options keep their order.
    let ids: Vec<String> = themes.picker.entries[4]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["b", "a"]);

    // -- Exec & Check: writing options does not open the picker.
    let mut unopened = AppState::default();
    unopened.set_picker_options(ActivePicker::Model, Vec::<PickerOption>::new());
    assert_eq!(unopened.picker.render_kind(), None);
    assert!(unopened.picker.entries[1].state.options.is_empty());

    // -- Setup & Fixtures: model options out of order.
    let mut models = AppState::default();
    models.set_picker_options(
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

    // -- Check: sorted by provider then identity.
    let ids: Vec<String> = models.picker.entries[1]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["a-provider/a-model", "z-provider/z-model"]);

    // -- Setup & Fixtures: configured and unconfigured models.
    let mut configured = AppState::default();
    configured.set_picker_options(
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

    // -- Check: configured models sort first.
    let ids: Vec<String> = configured.picker.entries[1]
        .state
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    assert_eq!(ids, vec!["openai/gpt-4o", "openai/gpt-4o-mini"]);

    // -- Exec & Check: opening the palette twice opens a single picker.
    let mut palette = AppState::default();
    open_command_palette_for_test(&mut palette);
    open_command_palette_for_test(&mut palette);
    assert_eq!(
        palette.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(palette.picker.active(), Some(ActivePicker::CommandPalette));
    Ok(())
}
