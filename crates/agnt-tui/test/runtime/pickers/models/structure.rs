use super::*;

#[tokio::test]
async fn model_picker_renders_column_header_row() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Model,
        vec![agnt_core::protocol::picker::ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            identity: "openai/gpt-4o".to_string(),
            display: "openai/gpt-4o".to_string(),
            active: true,
            context_window: Some(200000),
            max_output: None,
            configured: true,
            provider_display_name: String::new(),
            cost: Some("$3.00/$15.00".to_string()),
        }],
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
    let header_row = lines
        .iter()
        .position(|l| l.contains("PRICE"))
        .ok_or("should find the column header row")?;
    let data_row = lines
        .iter()
        .position(|l| l.contains("openai/gpt-4o"))
        .ok_or("should find the model data row")?;
    assert!(
        lines[header_row].contains("MODEL"),
        "header row must render the MODEL label; row: {:?}\n{popup_text}",
        lines[header_row]
    );
    assert!(
        lines[header_row].contains("CTX"),
        "header row must render the CTX label; row: {:?}\n{popup_text}",
        lines[header_row]
    );
    assert!(
        header_row < data_row,
        "header row must sit above the data row; header: {header_row}, data: {data_row}\n{popup_text}"
    );
    Ok(())
}

#[tokio::test]
async fn model_picker_renders_glyph_columns_at_row_end() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    let state = &mut driver.coordinator_mut().state;
    state.status.identity.active_model_identity = "openai/gpt-4o".to_string();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                identity: "openai/gpt-4o".to_string(),
                display: "openai/gpt-4o".to_string(),
                active: true,
                context_window: Some(200000),
                max_output: None,
                configured: true,
                provider_display_name: "OpenAI".to_string(),
                cost: Some("$3.00/$15.00".to_string()),
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
                provider_display_name: "Anthropic".to_string(),
                cost: None,
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
    let data_row = lines
        .iter()
        .position(|l| l.contains("openai/gpt-4o"))
        .ok_or("should find the model data row")?;
    let data_line = lines[data_row];
    let identity_at = data_line
        .find("openai/gpt-4o")
        .ok_or("data row should contain the identity")?;
    let active_at = data_line
        .find('*')
        .ok_or("data row should contain the active marker")?;
    let configured_at = data_line
        .find('◆')
        .ok_or("data row should contain the configured glyph")?;
    assert!(
        identity_at < active_at,
        "active marker must follow the identity; row: {data_line:?}\n{popup_text}"
    );
    assert!(
        identity_at < configured_at,
        "configured glyph must follow the identity; row: {data_line:?}\n{popup_text}"
    );
    assert!(
        data_line.ends_with("◆    │"),
        "selected+configured data line must end with the configured glyph; row: {data_line:?}\n{popup_text}"
    );
    let openai_group_row = lines
        .iter()
        .position(|l| l.contains("OpenAI"))
        .ok_or("should find the OpenAI provider group row")?;
    let anthropic_group_row = lines
        .iter()
        .position(|l| l.contains("Anthropic"))
        .ok_or("should find the Anthropic provider group row")?;
    assert!(
        lines[openai_group_row].contains("OpenAI"),
        "OpenAI group row must render the full provider name; row: {:?}\n{popup_text}",
        lines[openai_group_row]
    );
    assert!(
        lines[anthropic_group_row].contains("Anthropic"),
        "Anthropic group row must render the full provider name; row: {:?}\n{popup_text}",
        lines[anthropic_group_row]
    );
    Ok(())
}
