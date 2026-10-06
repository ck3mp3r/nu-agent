use super::*;

#[test]
fn model_picker_rows_do_not_contain_selection_prefix() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                identity: "openai/gpt-4o".to_string(),
                display: "openai/gpt-4o".to_string(),
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
                display: "anthropic/claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
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
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "default".to_string(),
                description: Some("Default agent".to_string()),
                display: "default".to_string(),
                builtin: true,
            },
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "coder".to_string(),
                description: Some("Coding assistant".to_string()),
                display: "coder".to_string(),
                builtin: false,
            },
            nu_agent_core::protocol::picker::AgentPickerOption {
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
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "reviewer".to_string(),
                description: Some("Runs reviews".to_string()),
                display: "reviewer".to_string(),
                builtin: false,
            },
            nu_agent_core::protocol::picker::AgentPickerOption {
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

#[tokio::test]
async fn model_picker_renders_active_and_configured_glyphs() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity = "openai/gpt-4o".to_string();
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
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
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic/claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let active_row = lines
        .iter()
        .position(|l| l.contains("openai/gpt-4o"))
        .ok_or("should find active model row")?;
    let configured_row = lines
        .iter()
        .position(|l| l.contains("openai/gpt-4o"))
        .ok_or("should find configured model row")?;
    let inactive_row = lines
        .iter()
        .position(|l| l.contains("anthropic/claude-3-5-sonnet"))
        .ok_or("should find inactive model row")?;
    assert!(
        lines[active_row].contains('*'),
        "active model row must render the active glyph; row: {:?}\n{popup_text}",
        lines[active_row]
    );
    assert!(
        lines[configured_row].contains('◆'),
        "configured model row must render the configured glyph; row: {:?}\n{popup_text}",
        lines[configured_row]
    );
    assert!(
        !lines[inactive_row].contains('*') && !lines[inactive_row].contains('◆'),
        "inactive+unconfigured row must render empty glyph cells; row: {:?}\n{popup_text}",
        lines[inactive_row]
    );
    Ok(())
}

#[tokio::test]
async fn model_picker_renders_context_window_column() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                identity: "openai/gpt-4o".to_string(),
                display: "openai/gpt-4o".to_string(),
                active: true,
                context_window: Some(200000),
                max_output: None,
                configured: true,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic/claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let ctx_row = lines
        .iter()
        .position(|l| l.contains("openai/gpt-4o"))
        .ok_or("should find context-window model row")?;
    let none_row = lines
        .iter()
        .position(|l| l.contains("anthropic/claude-3-5-sonnet"))
        .ok_or("should find no-context model row")?;
    assert!(
        lines[ctx_row].contains("200k"),
        "context-window row must render the 200k cell; row: {:?}\n{popup_text}",
        lines[ctx_row]
    );
    assert!(
        !lines[none_row].contains("200k"),
        "None-context row must render an empty context cell; row: {:?}\n{popup_text}",
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
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "default".to_string(),
                description: Some("Default agent".to_string()),
                display: "default".to_string(),
                builtin: true,
            },
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "coder".to_string(),
                description: Some("Coding assistant".to_string()),
                display: "coder".to_string(),
                builtin: false,
            },
            nu_agent_core::protocol::picker::AgentPickerOption {
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
