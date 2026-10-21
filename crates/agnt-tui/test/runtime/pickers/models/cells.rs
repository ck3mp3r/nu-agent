use super::*;

#[test]
fn model_picker_rows_do_not_contain_selection_prefix() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                identity: "openai/gpt-4o".to_string(),
                display: "openai/gpt-4o".to_string(),
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
                display: "anthropic/claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
                cost: None,
            },
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai/gpt-4o-mini".to_string(),
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
    if let Some(s) = state.picker.active_state_mut() {
        s.selection = 1;
    }

    let rows = super::model_picker_row_cells_for_test(&state);
    assert!(!rows.is_empty(), "expected at least one row");
    for row in &rows {
        for cell in row {
            assert!(
                !cell.starts_with("❯ "),
                "cell must not start with selection prefix '❯ ': {cell:?}"
            );
            assert!(
                !cell.starts_with("  "),
                "cell must not start with deselected prefix '  ': {cell:?}"
            );
        }
    }
}
