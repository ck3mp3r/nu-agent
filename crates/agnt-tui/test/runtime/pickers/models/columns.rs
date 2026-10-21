use super::*;

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
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                identity: "openai/gpt-4o".to_string(),
                display: "openai/gpt-4o".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: true,
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
            agnt_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                identity: "openai/gpt-4o".to_string(),
                display: "openai/gpt-4o".to_string(),
                active: true,
                context_window: Some(200000),
                max_output: None,
                configured: true,
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

#[tokio::test]
async fn model_picker_renders_cost_column() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
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
                provider_display_name: String::new(),
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
                provider_display_name: String::new(),
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
    let cost_row = lines
        .iter()
        .position(|l| l.contains("openai/gpt-4o"))
        .ok_or("should find priced model row")?;
    let none_row = lines
        .iter()
        .position(|l| l.contains("anthropic/claude-3-5-sonnet"))
        .ok_or("should find unpriced model row")?;
    assert!(
        lines[cost_row].contains("$3.00/$15.00"),
        "priced row must render the cost cell; row: {:?}\n{popup_text}",
        lines[cost_row]
    );
    assert!(
        !lines[none_row].contains('$'),
        "None-cost row must render an empty cost cell; row: {:?}\n{popup_text}",
        lines[none_row]
    );
    Ok(())
}

#[tokio::test]
async fn model_picker_renders_wide_cost_cell_untruncated() -> Result<()> {
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
            context_window: None,
            max_output: None,
            configured: true,
            provider_display_name: String::new(),
            cost: Some("$75.00/$150.00".to_string()),
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
    let cost_row = lines
        .iter()
        .position(|l| l.contains("openai/gpt-4o"))
        .ok_or("should find wide-cost model row")?;
    assert!(
        lines[cost_row].contains("$75.00/$150.00"),
        "wide cost cell must render untruncated; row: {:?}\n{popup_text}",
        lines[cost_row]
    );
    Ok(())
}

#[tokio::test]
async fn model_picker_renders_cost_cell_right_aligned() -> Result<()> {
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
    let data_row = lines
        .iter()
        .position(|l| l.contains("openai/gpt-4o"))
        .ok_or("should find the model data row")?;
    let header_row = lines
        .iter()
        .position(|l| l.contains("PRICE"))
        .ok_or("should find the column header row")?;
    assert!(
        lines[data_row].ends_with("$3.00/$15.00  *  ◆    │"),
        "data line must end with the right-aligned cost then the glyph columns; row: {:?}\n{popup_text}",
        lines[data_row]
    );
    assert!(
        lines[header_row].ends_with("PRICE          │"),
        "header line must end with the right-aligned PRICE label; row: {:?}\n{popup_text}",
        lines[header_row]
    );
    Ok(())
}

#[tokio::test]
async fn model_picker_renders_right_aligned_numeric_gutters() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(64, 30);
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity = "openai/gpt-4o".to_string();
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
    let area = ratatui::layout::Rect::new(0, 0, 64, 30);
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
    let header_row = lines
        .iter()
        .position(|l| l.contains("PRICE"))
        .ok_or("should find the column header row")?;
    let data_line = lines[data_row];
    assert!(
        data_line.ends_with("$3.00/$15.00  *  ◆    │"),
        "numeric cells must stay right-aligned with two-space glyph gutters at the minimum popup width; row: {data_line:?}\n{popup_text}"
    );
    let header_line = lines[header_row];
    assert!(
        header_line.ends_with("PRICE          │"),
        "header must keep the right-aligned PRICE label at the minimum popup width; row: {header_line:?}\n{popup_text}"
    );
    Ok(())
}

#[tokio::test]
async fn model_picker_renders_tight_context_price_gap() -> Result<()> {
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
    let data_row = lines
        .iter()
        .position(|l| l.contains("openai/gpt-4o"))
        .ok_or("should find the model data row")?;
    let data_line = lines[data_row];
    let ctx_end = data_line
        .find("200k")
        .map(|at| at + "200k".len())
        .ok_or("data row should contain the context window")?;
    let price_at = data_line
        .find("$3.00/$15.00")
        .ok_or("data row should contain the price")?;
    let gap = data_line
        .get(ctx_end..price_at)
        .ok_or("context window must precede the price")?;
    assert_eq!(
        gap, "      ",
        "CTX and PRICE must be separated by exactly six spaces; row: {data_line:?}\n{popup_text}"
    );
    Ok(())
}
