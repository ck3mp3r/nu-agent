//! Model picker footer identity: the selected identity in full, and the
//! footer pinned to the popup's bottom edge.

use super::*;

/// At 60x14 the popup inner height is 12, so the details ladder reserves two
/// rows below the table. The selected identity clips in its column but renders
/// in full in the footer.
#[tokio::test]
async fn test_model_picker_footer_shows_full_selected_identity() -> Result<()> {
    // -- Setup & Fixtures
    let identity = "openrouter/anthropic/claude-opus-4.5".to_string();
    let mut driver = RenderLoopDriver::new(62, 18);
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

    // -- Check: popup 60x14, inner 58x12, footer 2 rows. The column budget is
    // 60 - 34 = 26, so the row cell clips after 25 identity chars.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let clipped = format!("  {}…", identity.chars().take(25).collect::<String>());
    let table_idx = lines
        .iter()
        .position(|line| line.contains(&clipped))
        .ok_or("should render the ellipsized identity in the table")?;
    let footer_idx = lines
        .iter()
        .position(|line| line.contains(&identity))
        .ok_or("should render the full identity in the footer")?;
    assert!(
        table_idx < footer_idx,
        "the table row must sit above the footer; table: {table_idx}, footer: {footer_idx}\n{popup_text}"
    );
    Ok(())
}
/// At 60x20 the popup inner height is 18, so the details ladder caps the footer
/// at three rows. The one-row identity occupies the bottom inner row, directly
/// above the bottom border, and the table takes every row above it.
#[tokio::test]
async fn test_model_picker_footer_pins_to_bottom_edge_at_popup_height_twenty() -> Result<()> {
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

    // -- Check: popup 60x20 → inner 58x18 → ladder cap 3, and the identity wraps
    // to one row at width 53. The footer row is the bottom inner row (popup row
    // 18, the last row before the border at 19), with no reserved blank rows.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[18].contains(&identity),
        "the footer must sit on the bottom inner row (18); row: {:?}\n{popup_text}",
        lines[18]
    );
    assert!(
        !lines[16].contains(&identity) && !lines[17].contains(&identity),
        "no reserved blank rows may sit between the footer and the border; rows: {:?} {:?}\n{popup_text}",
        lines[16],
        lines[17]
    );
    Ok(())
}
