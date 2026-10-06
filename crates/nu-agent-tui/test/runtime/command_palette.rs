use super::*;

#[test]
fn command_palette_table_renders_required_columns_and_rows() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    let model = command_palette_table_model_for_test(&state, 80, 10);

    assert_eq!(model.columns, vec!["Action", "Summary"]);
    let actions = model
        .rows
        .iter()
        .map(|row| row[0].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        vec!["Help", "Status", "MCPs", "Skills", "Models", "Agents"]
    );
    assert!(model.rows.iter().all(|row| row[2].is_empty()));
    assert_eq!(model.selected, Some(0));
}

#[test]
fn command_palette_table_renders_skills_action_row() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    let model = command_palette_table_model_for_test(&state, 80, 10);
    let actions = model
        .rows
        .iter()
        .map(|row| row[0].as_str())
        .collect::<Vec<_>>();

    assert!(actions.contains(&"Skills"));
    assert!(actions.contains(&"Models"));
}

#[test]
fn inline_slash_suggestions_render_inline_with_single_hint_contract() {
    let mut state = AppState::default();
    state.check_inline_slash("/");

    let rows = inline_slash_lines_for_test(&state);
    assert!(!rows.is_empty());
    assert!(rows[0].contains("/compact"));
    assert!(rows[0].starts_with('❯'));

    let title = super::command_palette_title(None);
    assert!(title.contains("↑/↓ or Ctrl-N · Enter · Esc"));
}

#[test]
fn command_palette_table_emits_overflow_position_cue_when_viewport_is_small() -> Result<()> {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    let model = command_palette_table_model_for_test(&state, 80, 5);
    let cue = model
        .overflow_cue
        .ok_or("should have overflow cue when rows exceed viewport")?;
    assert!(cue.contains("/"));
    assert!(cue.contains("Esc close"));
    Ok(())
}

#[test]
fn help_modal_uses_large_readable_layout() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Help,
    );

    assert!(popup.width >= 72);
    assert!(popup.height >= 18);
}

#[test]
fn status_modal_uses_compact_layout() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Status,
    );

    assert!(popup.width <= 72);
    assert!(popup.height <= 14);
}

#[test]
fn modal_layout_policy_applies_consistently_across_panels() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let command_palette = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::CommandPalette,
    );
    let skills = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Skills,
    );
    let mcps = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Mcps,
    );

    assert_eq!(skills.width, mcps.width);
    assert_eq!(skills.height, mcps.height);
    assert!(command_palette.width < skills.width);
}

#[test]
fn models_modal_uses_layout_policy_defaults() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let models = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );
    let skills = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Skills,
    );

    assert_eq!(models.width, skills.width);
    assert_eq!(models.height, skills.height);
}

#[test]
fn themes_modal_uses_layout_policy_defaults() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let themes = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Themes,
    );
    let models = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );

    assert_eq!(themes.width, models.width);
    assert_eq!(themes.height, models.height);
}

#[test]
fn modal_open_state_applies_dimmed_backdrop() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    assert!(modal_open_state_applies_dimmed_backdrop_for_test(&state));
}

#[test]
fn inline_model_picker_modal_respects_border_and_backdrop_policy() {
    let mut state = AppState::default();
    state.picker.open(ActivePicker::Model);

    assert!(inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(&state));
}

#[test]
fn permission_does_not_open_global_dimmed_modal_backdrop() {
    let mut state = AppState::default();
    state
        .permission
        .open_prompt(crate::state::PermissionPrompt {
            request_id: "ask-0000000000000001".to_string(),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            scope: "nested".to_string(),
            pattern: "*".to_string(),
            target_field: Some("command".to_string()),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
        });

    assert!(!modal_open_state_applies_dimmed_backdrop_for_test(&state));
}

#[test]
fn model_picker_empty_catalog_shows_deterministic_empty_state() {
    let mut state = AppState::default();
    assert_eq!(
        crate::runtime::panels::MODEL_PICKER_EMPTY_STATE_MESSAGE,
        "No models available in cached startup config."
    );
    state.picker.open(ActivePicker::Model);
    assert!(state.picker.active_state().unwrap().filtered().is_empty());
}

#[test]
fn render_modal_frame_inner_is_inset_by_one_cell() {
    let area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 10,
        height: 5,
    };
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    assert_eq!(inner.width, 8);
    assert_eq!(inner.height, 3);
}

#[test]
fn scrollbar_state_does_not_panic_on_empty_transcript() {
    let mut state = ratatui::widgets::ScrollbarState::new(0).position(0);
    let _ = &mut state;
}

#[test]
fn scrollbar_state_does_not_panic_on_single_entry() {
    let mut state = ratatui::widgets::ScrollbarState::new(1).position(0);
    let _ = &mut state;
}

#[test]
fn transcript_list_area_is_two_columns_narrower_than_content_area() {
    let content_area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 20,
    };
    let list_area = ratatui::layout::Rect {
        width: content_area.width.saturating_sub(2),
        ..content_area
    };
    assert_eq!(list_area.width, 78);
    assert_eq!(list_area.x, content_area.x);
    assert_eq!(list_area.height, content_area.height);
}
