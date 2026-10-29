//! Model picker footer style: the fg-only text on the identity column, the gap
//! row above it, the base background at every popup height, and the alignment.

use super::*;

use crate::rendering::theme::TuiTheme;
use ratatui::style::Modifier;

/// The footer paints foreground-only text on the popup's base: no band, no bold,
/// and the first glyph starts on the identity column the model rows use, so the
/// footer reads as the selected row's detail instead of one more band.
#[tokio::test]
async fn test_model_picker_footer_renders_fg_only_text_at_the_identity_column() -> Result<()> {
    // -- Setup & Fixtures
    let identity = "openrouter/anthropic/claude-opus-4.5".to_string();
    let theme = TuiTheme::default();
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

    // -- Check: popup 60x20 at the origin, so the one-row identity footer sits on
    // popup row 18. The model rows put the identity on popup column 5, and the
    // footer text starts on that column with no background band around it.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let footer_row = popup.y + popup.height - 2;
    let leading_style = driver.cell_style_at(popup.x + 4, footer_row);
    let text_style = driver.cell_style_at(popup.x + 5, footer_row);
    let trailing_style = driver.cell_style_at(popup.x + popup.width - 3, footer_row);
    assert_eq!(
        lines[18].chars().nth(5),
        Some('o'),
        "the footer text must start on the identity column; row: {:?}\n{popup_text}",
        lines[18]
    );
    assert_eq!(
        text_style.fg, theme.subtle_meta.fg,
        "the footer text must carry the popup's meta foreground; style: {text_style:?}"
    );
    assert!(
        !text_style.add_modifier.contains(Modifier::BOLD),
        "the footer text must not be bold; style: {text_style:?}"
    );
    assert_eq!(
        text_style.bg,
        Some(theme.base),
        "the footer text must sit on the popup base, not a band; style: {text_style:?}"
    );
    assert_eq!(
        leading_style.bg,
        Some(theme.base),
        "no band may start before the footer text; style: {leading_style:?}"
    );
    assert_eq!(
        trailing_style.bg,
        Some(theme.base),
        "no band may fill the footer's trailing blanks; style: {trailing_style:?}"
    );
    Ok(())
}
/// At 60x20 a twenty-model list fills the table area, so the last data row would
/// sit flush against the footer text. The footer text takes the bottom inner row
/// and the row directly above it stays blank base background, so the footer
/// never touches the list.
#[tokio::test]
async fn test_model_picker_footer_leaves_a_gap_row_above_the_text() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let options = (0..20)
        .map(|idx| agnt_core::protocol::picker::ModelPickerOption {
            provider: "openrouter".to_string(),
            model: format!("model-{idx}"),
            identity: format!("openrouter/model-{idx}"),
            display: format!("openrouter/model-{idx}"),
            active: false,
            context_window: None,
            max_output: None,
            configured: false,
            provider_display_name: "OpenRouter".to_string(),
            cost: None,
        })
        .collect::<Vec<_>>();
    let identity = "openrouter/model-0".to_string();
    let mut driver = RenderLoopDriver::new(62, 24);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Model, options);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 60x20 → inner 58x18 → the one-row identity fills one text
    // row, so the footer claims rows 17 and 18: row 17 is the blank gap and row
    // 18 carries the footer text on the bottom inner row.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[17].trim_matches(['│', ' ']).is_empty(),
        "the gap row above the footer text must carry no text; row: {:?}\n{popup_text}",
        lines[17]
    );
    assert_eq!(
        driver.cell_style_at(popup.x + 5, 17).bg,
        Some(theme.base),
        "the gap row must keep the base background; popup:\n{popup_text}"
    );
    assert!(
        !lines[16].trim_matches(['│', ' ']).is_empty(),
        "the table must fill every row above the gap; row: {:?}\n{popup_text}",
        lines[16]
    );
    assert!(
        lines[18].contains(&identity),
        "the footer text must still sit on the bottom inner row (18); row: {:?}\n{popup_text}",
        lines[18]
    );
    Ok(())
}
/// At every designed popup height the footer row keeps the popup base
/// background and a foreground-only text style, so the footer never paints a
/// band across the popup.
#[tokio::test]
async fn test_model_picker_footer_renders_fg_only_text_at_every_popup_height() -> Result<()> {
    // -- Setup & Fixtures
    let identity = "openrouter/anthropic/claude-opus-4.5".to_string();
    let theme = TuiTheme::default();

    // -- Exec & Check
    for height in 10..=20u16 {
        let mut driver = RenderLoopDriver::new(62, height + 4);
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
        driver.advance_with_frame(&[]).await?;

        let popup = super::render::frame::modal_rect_for_panel(
            driver.state().scroll.transcript_area,
            super::render::frame::ModalPanelKind::Models,
        );
        let footer_row = popup.y + popup.height - 2;
        let text_style = driver.cell_style_at(popup.x + 5, footer_row);
        let row_bg = driver.cell_style_at(popup.x + 5, footer_row - 1).bg;
        assert_eq!(
            text_style.fg, theme.subtle_meta.fg,
            "popup height {height}: the footer text must carry the meta foreground; style: {text_style:?}"
        );
        assert!(
            !text_style.add_modifier.contains(Modifier::BOLD),
            "popup height {height}: the footer text must not be bold; style: {text_style:?}"
        );
        assert_eq!(
            text_style.bg,
            Some(theme.base),
            "popup height {height}: the footer row must keep the popup base, not a band; style: {text_style:?}"
        );
        assert_eq!(
            row_bg,
            Some(theme.base),
            "popup height {height}: the row above the footer must keep the popup base"
        );
    }
    Ok(())
}
/// At 60x20 the footer text starts on the same column as the model rows'
/// identity text, so the popup has one left edge for its data and its footer.
#[tokio::test]
async fn test_model_picker_footer_aligns_the_text_with_the_identity_column() -> Result<()> {
    // -- Setup & Fixtures
    let identity = "openrouter/anthropic/claude-opus-4.5".to_string();
    let theme = TuiTheme::default();
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

    // -- Check: popup 60x20 at the origin, so the one-row identity footer sits on
    // popup row 18. The table's identity cell carries the two-cell selection
    // gutter plus its two-space indent, and the footer starts on that column.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let table_row = lines
        .iter()
        .position(|line| line.contains("openrouter/"))
        .ok_or("should render the model row")?;
    let footer_row = lines
        .iter()
        .rposition(|line| line.contains(&identity))
        .ok_or("should render the full identity in the footer")?;
    let table_at = lines[table_row]
        .find("openrouter/")
        .ok_or("the model row should contain the identity")?;
    let table_col = lines[table_row][..table_at].chars().count();
    let footer_at = lines[footer_row]
        .find("openrouter/")
        .ok_or("the footer row should contain the identity")?;
    let footer_col = lines[footer_row][..footer_at].chars().count();
    assert_eq!(
        footer_col, table_col,
        "the footer must start on the identity column; table: {table_col}, footer: {footer_col}\n{popup_text}"
    );
    assert_eq!(
        footer_col, 5,
        "the identity column is the border, the inner edge, the two-cell gutter, and the two-space indent; column: {footer_col}\n{popup_text}"
    );
    let leading = driver.cell_style_at(popup.x + 4, popup.y + popup.height - 2);
    assert_eq!(
        leading.bg,
        Some(theme.base),
        "no band may start before the footer text; style: {leading:?}"
    );
    Ok(())
}
