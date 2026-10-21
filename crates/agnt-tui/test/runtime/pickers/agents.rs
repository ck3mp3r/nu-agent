use super::*;

#[tokio::test]
async fn agent_picker_renders_active_marker_from_identity() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_agent_identity = Some("coder".to_string());
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Agent,
        vec![
            agnt_core::protocol::picker::AgentPickerOption {
                name: "default".to_string(),
                description: Some("Default agent".to_string()),
                display: "default".to_string(),
                builtin: true,
            },
            agnt_core::protocol::picker::AgentPickerOption {
                name: "coder".to_string(),
                description: Some("Coding assistant".to_string()),
                display: "coder".to_string(),
                builtin: false,
            },
            agnt_core::protocol::picker::AgentPickerOption {
                name: "reviewer".to_string(),
                description: None,
                display: "reviewer".to_string(),
                builtin: false,
            },
        ],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Agents,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let active_row = lines
        .iter()
        .position(|l| l.contains("coder"))
        .ok_or("should find active agent row")?;
    let inactive_row = lines
        .iter()
        .position(|l| l.contains("default"))
        .ok_or("should find inactive agent row")?;
    assert!(
        lines[active_row].contains('*'),
        "active agent row must render the marker; row: {:?}\n{popup_text}",
        lines[active_row]
    );
    assert!(
        !lines[inactive_row].contains('*'),
        "inactive agent row must render an empty marker cell; row: {:?}\n{popup_text}",
        lines[inactive_row]
    );
    Ok(())
}

#[tokio::test]
async fn agent_picker_renders_description_column() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Agent,
        vec![
            agnt_core::protocol::picker::AgentPickerOption {
                name: "reviewer".to_string(),
                description: Some("Runs reviews".to_string()),
                display: "reviewer".to_string(),
                builtin: false,
            },
            agnt_core::protocol::picker::AgentPickerOption {
                name: "plain".to_string(),
                description: None,
                display: "plain".to_string(),
                builtin: false,
            },
        ],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Agents,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let desc_row = lines
        .iter()
        .position(|l| l.contains("reviewer"))
        .ok_or("should find reviewer row")?;
    let none_row = lines
        .iter()
        .position(|l| l.contains("plain"))
        .ok_or("should find plain row")?;
    assert!(
        lines[desc_row].contains("Runs reviews"),
        "description row must render the description text; row: {:?}\n{popup_text}",
        lines[desc_row]
    );
    assert!(
        !lines[none_row].contains("Runs reviews"),
        "None-description row must not render a description; row: {:?}\n{popup_text}",
        lines[none_row]
    );
    Ok(())
}

#[test]
fn agent_picker_rows_do_not_contain_selection_prefix() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![
            agnt_core::protocol::picker::AgentPickerOption {
                name: "default".to_string(),
                description: Some("Default agent".to_string()),
                display: "default".to_string(),
                builtin: true,
            },
            agnt_core::protocol::picker::AgentPickerOption {
                name: "coder".to_string(),
                description: Some("Coding assistant".to_string()),
                display: "coder".to_string(),
                builtin: false,
            },
            agnt_core::protocol::picker::AgentPickerOption {
                name: "reviewer".to_string(),
                description: None,
                display: "reviewer".to_string(),
                builtin: false,
            },
        ],
    );
    state.picker.open(ActivePicker::Agent);
    if let Some(s) = state.picker.active_state_mut() {
        s.selection = 1;
    }

    let rows = super::agent_picker_row_cells_for_test(&state);
    assert!(!rows.is_empty(), "expected at least one row");
    for row in &rows {
        for cell in row {
            assert!(
                !cell.starts_with("  "),
                "cell must not start with deselected prefix '  ': {cell:?}"
            );
        }
    }
}
