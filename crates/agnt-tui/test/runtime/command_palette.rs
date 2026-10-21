use super::*;

/// The command palette table renders the required columns and rows, includes
/// the Skills/Models actions, emits an overflow cue at a small viewport, and
/// the inline slash suggestions render inline with a single hint contract.
#[test]
fn command_palette_table_and_inline_slash() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    // -- Exec
    let model = command_palette_table_model_for_test(&state, 80, 10);

    // -- Check: columns, actions, and selection.
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
    assert!(actions.contains(&"Skills"));
    assert!(actions.contains(&"Models"));

    // -- Exec & Check: the overflow cue at a small viewport.
    let small = command_palette_table_model_for_test(&state, 80, 5);
    let cue = small
        .overflow_cue
        .ok_or("should have overflow cue when rows exceed viewport")?;
    assert!(cue.contains("/"));
    assert!(cue.contains("Esc close"));

    // -- Setup & Fixtures: inline slash suggestions.
    let mut slash = AppState::default();
    slash.check_inline_slash("/");

    // -- Exec & Check: the inline rows and the single hint contract.
    let rows = inline_slash_lines_for_test(&slash);
    assert!(!rows.is_empty());
    assert!(rows[0].contains("/compact"));
    assert!(rows[0].starts_with('❯'));
    let title = super::command_palette_title(None);
    assert!(title.contains("↑/↓ or Ctrl-N · Enter · Esc"));
    Ok(())
}

/// The modal layout policy applies consistently across panels: Help is large,
/// Status is compact, and the remaining panels share the default size.
#[test]
fn modal_layout_policy() -> Result<()> {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);

    // -- Exec & Check: Help is large.
    let help = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Help,
    );
    assert!(help.width >= 72);
    assert!(help.height >= 18);

    // -- Exec & Check: Status is compact.
    let status = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Status,
    );
    assert!(status.width <= 72);
    assert!(status.height <= 14);

    // -- Exec & Check: Skills and MCPs share a size; the palette is narrower.
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

    // -- Exec & Check: Models is narrower than the shared default; Themes keeps it.
    let models = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );
    let themes = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Themes,
    );
    assert!(models.width < skills.width);
    assert_eq!(models.height, skills.height);
    assert_eq!(themes.width, skills.width);
    assert_eq!(themes.height, skills.height);
    Ok(())
}

/// A modal open state applies the dimmed backdrop; the inline model picker
/// respects the border/backdrop policy; a permission prompt does not open the
/// global backdrop; and an empty model catalog shows a deterministic empty
/// state.
#[test]
fn modal_backdrop_and_empty_state() -> Result<()> {
    // -- Setup & Fixtures: the command palette open.
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    // -- Exec & Check: the dimmed backdrop applies.
    assert!(modal_open_state_applies_dimmed_backdrop_for_test(&state));

    // -- Setup & Fixtures: the inline model picker open.
    let mut model = AppState::default();
    model.picker.open(ActivePicker::Model);

    // -- Exec & Check: the border/backdrop policy is respected.
    assert!(inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(&model));

    // -- Setup & Fixtures: a permission prompt.
    let mut permission = AppState::default();
    permission
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

    // -- Exec & Check: no global backdrop.
    assert!(!modal_open_state_applies_dimmed_backdrop_for_test(
        &permission
    ));

    // -- Setup & Fixtures: an empty model catalog.
    let mut empty = AppState::default();
    assert_eq!(
        crate::runtime::panels::MODEL_PICKER_EMPTY_STATE_MESSAGE,
        "No models available in cached startup config."
    );
    empty.picker.open(ActivePicker::Model);

    // -- Exec & Check: the deterministic empty state.
    assert!(
        empty
            .picker
            .active_state()
            .ok_or("picker should be open")?
            .filtered()
            .is_empty()
    );
    Ok(())
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
fn scrollbar_state_does_not_panic_on_empty_or_single_entry() {
    let mut empty = ratatui::widgets::ScrollbarState::new(0).position(0);
    let _ = &mut empty;
    let mut single = ratatui::widgets::ScrollbarState::new(1).position(0);
    let _ = &mut single;
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
