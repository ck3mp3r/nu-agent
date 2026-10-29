use super::*;

use crate::state::{PickerOption, PickerPayload};

/// A theme display name wider than the Theme column renders ellipsized inside
/// the popup at the minimum popup width.
#[tokio::test]
async fn theme_picker_ellipsizes_long_display_name_at_narrow_width() -> Result<()> {
    // -- Setup & Fixtures
    let display = "t".repeat(60);
    let mut driver = RenderLoopDriver::new(56, 20);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Theme, vec![theme_option(&display)]);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Theme);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup width 32 → table width 30 → theme budget 30 - 3 = 27.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Themes,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains('❯'))
        .ok_or("should render the selected theme row")?;
    let clipped = format!("{}…", "t".repeat(26));
    assert!(
        row.contains(&clipped),
        "the theme row should carry the ellipsized display name {clipped:?}; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

/// A theme display name exactly as wide as the Theme column budget renders
/// unellipsized: the table spends no width on a dead marker column.
#[tokio::test]
async fn theme_picker_renders_budget_width_display_name_unellipsized() -> Result<()> {
    // -- Setup & Fixtures
    let display = "t".repeat(27);
    let mut driver = RenderLoopDriver::new(56, 20);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Theme, vec![theme_option(&display)]);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Theme);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup width 32 → table width 30 → theme budget 30 - 3 = 27.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Themes,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains('❯'))
        .ok_or("should render the selected theme row")?;
    assert!(
        row.contains(&display),
        "a budget-width display name must render in full; row: {row:?}\n{popup_text}"
    );
    assert!(
        !row.contains('…'),
        "a budget-width display name must not be ellipsized; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

/// A theme display name that fits the Theme column renders unchanged.
#[tokio::test]
async fn theme_picker_keeps_fitting_display_name_at_narrow_width() -> Result<()> {
    // -- Setup & Fixtures
    let display = "Catppuccin Macchiato".to_string();
    let mut driver = RenderLoopDriver::new(56, 20);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Theme, vec![theme_option(&display)]);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Theme);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Themes,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains("Catppuccin Macchiato"))
        .ok_or("should render the theme row")?;
    assert!(
        !row.contains('…'),
        "a fitting display name must not be ellipsized; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

/// The themes popup fits its content: four short single-column names and no
/// footer do not need the agents/sessions surface.
#[tokio::test]
async fn theme_picker_popup_is_right_sized_to_its_content() -> Result<()> {
    // -- Setup & Fixtures
    let names = [
        "Catppuccin Mocha",
        "Catppuccin Latte",
        "Catppuccin Frappe",
        "Catppuccin Macchiato",
    ];
    let mut driver = RenderLoopDriver::new(56, 20);
    driver.coordinator_mut().state.set_picker_options(
        ActivePicker::Theme,
        names.iter().map(|name| theme_option(name)).collect(),
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Theme);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the popup clamps to its own (28, 32, 8, 12) range, not the
    // agents/sessions (56, 90, 10, 20) range, and every theme row is visible.
    let (x, y, width, height) =
        rendered_popup_rect(&driver).ok_or("should render the themes popup border")?;
    assert_eq!(
        (width, height),
        (32, 12),
        "the themes popup must be right-sized to its content; rendered:\n{}",
        driver.buffer_text()
    );
    let popup_text = driver.buffer_text_in_rect(ratatui::layout::Rect::new(x, y, width, height));
    for name in names {
        assert!(
            popup_text.contains(name),
            "every theme row must be visible in the popup; missing {name:?} in:\n{popup_text}"
        );
    }
    Ok(())
}

// region:    --- Test Support

fn theme_option(display: &str) -> PickerOption {
    PickerOption {
        id: display.to_string(),
        display: display.to_string(),
        search_text: display.to_string(),
        sort_key: Vec::new(),
        payload: PickerPayload::Theme,
    }
}

// endregion: --- Test Support
