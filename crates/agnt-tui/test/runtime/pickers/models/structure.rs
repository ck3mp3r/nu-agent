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

#[tokio::test]
async fn model_picker_paints_section_header_bar() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Model,
        vec![
            model_option("ollama", "llama3", "Ollama"),
            model_option("ollama", "qwen2", "Ollama"),
            model_option("openai", "gpt-4o", "OpenAI"),
            model_option("openai", "gpt-4o-mini", "OpenAI"),
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
    let palette = crate::rendering::theme::CatppuccinPalette::mocha();
    let theme = crate::rendering::theme::TuiTheme::default();
    let inner_x = popup.x + 1..=popup.x + popup.width - 2;

    for name in ["Ollama", "OpenAI"] {
        let row = lines
            .iter()
            .position(|line| line.contains(name))
            .ok_or(format!("should find the {name} section header row"))?;
        let y = popup.y + u16::try_from(row)?;
        for x in inner_x.clone() {
            assert_eq!(
                driver.cell_style_at(x, y).bg,
                Some(palette.surface1),
                "{name} header bar must cover x={x} on row {row}\n{popup_text}"
            );
        }
        let name_at = lines[row]
            .find(name)
            .ok_or(format!("{name} header row should contain the display name"))?;
        let name_x = popup.x + u16::try_from(name_at)?;
        assert_eq!(
            driver.cell_style_at(name_x, y).fg,
            Some(palette.subtext0),
            "{name} header text must render in subtext0\n{popup_text}"
        );
    }

    let plain_rows = [
        (
            "query",
            lines
                .iter()
                .position(|line| line.contains("Query:"))
                .ok_or("should find the query row")?,
        ),
        (
            "column header",
            lines
                .iter()
                .position(|line| line.contains("PRICE"))
                .ok_or("should find the column header row")?,
        ),
        (
            "model",
            lines
                .iter()
                .position(|line| line.contains("ollama/llama3"))
                .ok_or("should find a model row")?,
        ),
    ];
    for (label, row) in plain_rows {
        let y = popup.y + u16::try_from(row)?;
        for x in inner_x.clone() {
            assert_eq!(
                driver.cell_style_at(x, y).bg,
                Some(theme.base),
                "{label} row must keep the base background at x={x} on row {row}\n{popup_text}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn model_picker_renders_bold_section_titles_left_of_model_rows() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Model,
        vec![
            model_option("ollama", "llama3", "Ollama"),
            model_option("openai", "gpt-4o", "OpenAI"),
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

    for name in ["Ollama", "OpenAI"] {
        let row = lines
            .iter()
            .position(|line| line.contains(name))
            .ok_or(format!("should find the {name} section header row"))?;
        let y = popup.y + u16::try_from(row)?;
        let name_at = lines[row]
            .find(name)
            .ok_or(format!("{name} header row should contain the display name"))?;
        let name_col = lines[row][..name_at].chars().count();
        let name_x = popup.x + u16::try_from(name_col)?;
        assert_eq!(
            name_x,
            popup.x + 4,
            "{name} title must start one character after the selection gutter\n{popup_text}"
        );
        assert!(
            driver
                .cell_style_at(name_x, y)
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD),
            "{name} title must render bold\n{popup_text}"
        );
    }

    let model_row = lines
        .iter()
        .position(|line| line.contains("ollama/llama3"))
        .ok_or("should find a model row")?;
    let identity_at = lines[model_row]
        .find("ollama/llama3")
        .ok_or("model row should contain the identity")?;
    let identity_col = lines[model_row][..identity_at].chars().count();
    let identity_x = popup.x + u16::try_from(identity_col)?;
    assert_eq!(
        identity_x,
        popup.x + 5,
        "model identity must keep its two-space prefix\n{popup_text}"
    );

    let title_row = lines
        .iter()
        .position(|line| line.contains("Ollama"))
        .ok_or("should find the Ollama section header row")?;
    let title_at = lines[title_row]
        .find("Ollama")
        .ok_or("Ollama header row should contain the display name")?;
    let title_col = lines[title_row][..title_at].chars().count();
    let title_x = popup.x + u16::try_from(title_col)?;
    assert_eq!(
        identity_x,
        title_x + 1,
        "section title must sit one character left of the model identity\n{popup_text}"
    );
    Ok(())
}

fn model_option(
    provider: &str,
    model: &str,
    provider_display_name: &str,
) -> agnt_core::protocol::picker::ModelPickerOption {
    agnt_core::protocol::picker::ModelPickerOption {
        provider: provider.to_string(),
        model: model.to_string(),
        identity: format!("{provider}/{model}"),
        display: format!("{provider}/{model}"),
        active: false,
        context_window: None,
        max_output: None,
        configured: true,
        provider_display_name: provider_display_name.to_string(),
        cost: None,
    }
}
