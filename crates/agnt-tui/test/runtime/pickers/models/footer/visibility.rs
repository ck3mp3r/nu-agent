//! Model picker footer visibility: the popup heights and states where the
//! details ladder reserves no footer row at all.

use super::*;

/// Below the first rung of the details ladder the pickers reserve no footer
/// row, so a fitting identity appears only in the table.
#[tokio::test]
async fn test_model_picker_footer_absent_at_inner_height_six() -> Result<()> {
    // -- Setup & Fixtures
    let identity = "openai/gpt-4o".to_string();
    let mut driver = RenderLoopDriver::new(62, 12);
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

    // -- Check: popup height 8 → inner height 6 → details height 0.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let occurrences = popup_text
        .lines()
        .filter(|line| line.contains(&identity))
        .count();
    assert_eq!(
        occurrences, 1,
        "a fitting identity must appear once, in the table only; popup:\n{popup_text}"
    );
    Ok(())
}
/// With no options there is no selected option, so the picker reserves no footer
/// rows at all and the empty state takes the whole area below the query row.
#[tokio::test]
async fn test_model_picker_footer_absent_without_selection() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(62, 18);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 60x14, inner 58x12. No footer text, so the footer rect is
    // empty, the table area holds every inner row, and the popup rows below the
    // empty state stay blank.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[2].contains("No models available"),
        "the empty state must render on the first content row; row: {:?}\n{popup_text}",
        lines[2]
    );
    let footer_rows: Vec<&str> = lines.iter().skip(11).take(2).copied().collect();
    for row in footer_rows {
        assert!(
            row.trim_matches(['│', ' ']).is_empty(),
            "no footer text may render without a selection; row: {row:?}\n{popup_text}"
        );
    }
    Ok(())
}
