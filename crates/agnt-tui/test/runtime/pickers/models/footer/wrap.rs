//! Model picker footer wrap: the two-row footer text at the two popup heights
//! where the details ladder reserves two rows.

use super::*;

/// At 60x19 the popup inner height is 17, so the ladder still caps the footer at
/// three rows. A footer text that wraps to two rows occupies the bottom two
/// inner rows with every character visible.
#[tokio::test]
async fn test_model_picker_footer_wraps_to_bottom_two_rows_at_popup_height_nineteen() -> Result<()>
{
    // -- Setup & Fixtures
    let identity = format!("openrouter/{}", "a".repeat(60));
    let mut driver = RenderLoopDriver::new(62, 23);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Model,
        vec![agnt_core::protocol::picker::ModelPickerOption {
            provider: "openrouter".to_string(),
            model: identity.clone(),
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

    // -- Check: popup 60x19 → inner 58x17 → ladder cap 3, and the 71-cell
    // identity wraps to two rows at width 53. Both rows are the bottom two inner
    // rows (popup rows 16 and 17), with no characters dropped.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let footer_rows: Vec<String> = lines
        .iter()
        .skip(16)
        .take(2)
        .map(|line| line.replace('│', ""))
        .collect();
    assert!(
        footer_rows[0].contains("openrouter/"),
        "the first footer row must carry the provider prefix; row: {:?}\n{popup_text}",
        footer_rows[0]
    );
    let footer_identity_chars = footer_rows
        .iter()
        .flat_map(|row| row.chars())
        .filter(|c| *c == 'a')
        .count();
    assert_eq!(
        footer_identity_chars, 60,
        "the bottom two rows must carry every identity character; rows: {footer_rows:?}\n{popup_text}"
    );
    assert!(
        !lines[15].contains('a'),
        "no footer text may sit above the bottom two rows; row: {:?}\n{popup_text}",
        lines[15]
    );
    Ok(())
}
/// At terminal 60x10 the popup inner height is 8, so the details ladder
/// reserves two footer rows. The selected identity is wider than the inner
/// width, and both wrapped rows render with no characters dropped.
#[tokio::test]
async fn test_model_picker_footer_wraps_to_two_rows_at_inner_height_eight() -> Result<()> {
    // -- Setup & Fixtures
    let identity = format!("openrouter/{}", "a".repeat(60));
    let mut driver = RenderLoopDriver::new(62, 14);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Model,
        vec![agnt_core::protocol::picker::ModelPickerOption {
            provider: "openrouter".to_string(),
            model: identity.clone(),
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

    // -- Check: popup 60x10, inner 58x8. The details ladder grants two rows at
    // popup rows 7 and 8, so the 71-cell identity wraps at width 53.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let footer_rows: Vec<String> = lines
        .iter()
        .skip(7)
        .take(2)
        .map(|line| line.replace('│', ""))
        .collect();
    assert_eq!(footer_rows.len(), 2, "popup must expose both footer rows");
    assert!(
        footer_rows[0].contains("openrouter/"),
        "the first footer row must carry the provider prefix; row: {:?}\n{popup_text}",
        footer_rows[0]
    );
    let footer_identity_chars = footer_rows
        .iter()
        .flat_map(|row| row.chars())
        .filter(|c| *c == 'a')
        .count();
    assert_eq!(
        footer_identity_chars, 60,
        "the two footer rows must carry every identity character; rows: {footer_rows:?}\n{popup_text}"
    );
    for row in &footer_rows {
        assert!(
            !row.trim().is_empty(),
            "both footer rows must render text; row: {row:?}\n{popup_text}"
        );
        assert!(
            !row.contains('…'),
            "the footer must not truncate with a marker; row: {row:?}\n{popup_text}"
        );
    }
    Ok(())
}
