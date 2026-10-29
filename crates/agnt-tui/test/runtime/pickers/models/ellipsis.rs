use super::*;

/// A model identity wider than the Model column renders ellipsized inside the
/// popup at the minimum popup width.
#[tokio::test]
async fn model_picker_ellipsizes_long_identity_at_narrow_width() -> Result<()> {
    // -- Setup & Fixtures
    let identity = "openrouter/anthropic/claude-opus-4.5".to_string();
    let mut driver = RenderLoopDriver::new(62, 24);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Model,
        vec![agnt_core::protocol::picker::ModelPickerOption {
            provider: "openrouter".to_string(),
            model: "anthropic/claude-opus-4.5".to_string(),
            identity: identity.clone(),
            display: identity.clone(),
            active: true,
            context_window: None,
            max_output: None,
            configured: false,
            provider_display_name: String::new(),
            cost: None,
        }],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup width 60 leaves 60 - 34 = 26 chars for the identity, so
    // the cell keeps the 2-space indent plus 25 identity chars and a trailing
    // ellipsis.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let clipped = format!("  {}…", identity.chars().take(25).collect::<String>());
    assert!(
        popup_text.contains(&clipped),
        "the model row should carry the ellipsized identity {clipped:?}\n{popup_text}"
    );
    Ok(())
}

/// A model identity that fits the Model column renders unchanged.
#[tokio::test]
async fn model_picker_keeps_fitting_identity_at_narrow_width() -> Result<()> {
    // -- Setup & Fixtures
    let identity = "openai/gpt-4o".to_string();
    let mut driver = RenderLoopDriver::new(62, 24);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Model,
        vec![agnt_core::protocol::picker::ModelPickerOption {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            identity: identity.clone(),
            display: identity.clone(),
            active: true,
            context_window: None,
            max_output: None,
            configured: false,
            provider_display_name: String::new(),
            cost: None,
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
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains("openai/gpt-4o"))
        .ok_or("should render the model row")?;
    assert!(
        !row.contains('…'),
        "a fitting identity must not be ellipsized; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

/// A priced, context-windowed option renders both the CTX and PRICE columns at
/// a popup produced by a narrow terminal (60x10 → 58x6 transcript pane → popup
/// 58x6).
#[tokio::test]
async fn model_picker_shows_ctx_and_price_columns_at_60x10() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(60, 10);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Model, vec![priced_model_option()]);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    assert_ctx_and_price_columns(&driver)?;
    Ok(())
}

/// A priced, context-windowed option renders both the CTX and PRICE columns at
/// the popup's minimum width with a two-row footer (60x14 → 58x10 transcript
/// pane → popup 58x10).
#[tokio::test]
async fn model_picker_shows_ctx_and_price_columns_at_60x14() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(60, 14);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Model, vec![priced_model_option()]);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    assert_ctx_and_price_columns(&driver)?;
    Ok(())
}

/// A priced, context-windowed option renders both the CTX and PRICE columns at
/// the popup's maximum width (80x20 → 78x16 transcript pane → popup 78x16).
#[tokio::test]
async fn model_picker_shows_ctx_and_price_columns_at_80x20() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(80, 20);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Model, vec![priced_model_option()]);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    assert_ctx_and_price_columns(&driver)?;
    Ok(())
}

// -- Test Support

/// A model option carrying a 200k context window and a price.
fn priced_model_option() -> agnt_core::protocol::picker::ModelPickerOption {
    agnt_core::protocol::picker::ModelPickerOption {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        identity: "openai/gpt-4o".to_string(),
        display: "openai/gpt-4o".to_string(),
        active: true,
        context_window: Some(200_000),
        max_output: None,
        configured: false,
        provider_display_name: String::new(),
        cost: Some("$3.00/MTok".to_string()),
    }
}

/// Asserts the popup rendered by the driver carries the CTX and PRICE header
/// labels and their data cells.
fn assert_ctx_and_price_columns(driver: &RenderLoopDriver) -> Result<()> {
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    for needle in ["CTX", "PRICE", "200k", "$3.00/MTok"] {
        assert!(
            popup_text.contains(needle),
            "the models popup must render {needle:?}; popup:\n{popup_text}"
        );
    }
    Ok(())
}
