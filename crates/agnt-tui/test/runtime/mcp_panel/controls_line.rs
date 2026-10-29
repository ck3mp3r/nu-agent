use unicode_width::UnicodeWidthStr;

use super::*;

/// At the popup's minimum width the controls line ellipsizes to the inner width
/// with a trailing marker instead of hard-clipping at the frame border.
#[tokio::test]
async fn mcp_panel_controls_line_ellipsizes_at_minimum_popup_width() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(58, 20);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup inner width 54 holds 53 cells of the line plus the marker.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    assert_eq!(
        popup.width, 56,
        "the MCP popup must render at the 56-column minimum"
    );
    let controls = controls_row_text(&driver.buffer_text_in_rect(popup))?;
    assert_eq!(
        UnicodeWidthStr::width(controls.as_str()),
        54,
        "the controls line must fill the 54-cell inner width; line: {controls:?}"
    );
    assert!(
        controls.ends_with('…'),
        "the controls line must ellipsize with a marker; line: {controls:?}"
    );
    let head = controls.trim_end_matches('…');
    assert!(
        mcp_panel_controls_line().starts_with(head),
        "the ellipsized line must carry the head of the controls text {head:?}"
    );
    Ok(())
}

/// At the narrowest popup width that holds the whole controls line, the line
/// renders untruncated and carries no marker.
#[tokio::test]
async fn mcp_panel_controls_line_renders_whole_at_designed_minimum() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(64, 20);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup inner width 60 equals the line width.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    assert_eq!(
        popup.width, 62,
        "the MCP popup must widen with a 62-column terminal"
    );
    let controls = controls_row_text(&driver.buffer_text_in_rect(popup))?;
    assert_eq!(
        controls,
        mcp_panel_controls_line(),
        "the whole controls line must render at inner width 60"
    );
    Ok(())
}

/// At the popup's minimum width an active filter query that overflows the inner
/// width ellipsizes to the inner width with a trailing marker instead of
/// hard-clipping at the frame border.
#[tokio::test]
async fn mcp_panel_controls_line_ellipsizes_long_query_at_minimum_popup_width() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(58, 20);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);
    for ch in "g".repeat(80).chars() {
        driver
            .coordinator_mut()
            .state
            .status
            .mcp
            .append_mcp_panel_query_char(ch);
    }

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup inner width 54 holds 53 cells of the line plus the marker.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let controls = controls_row_text(&driver.buffer_text_in_rect(popup))?;
    assert_eq!(
        UnicodeWidthStr::width(controls.as_str()),
        54,
        "the query line must fill the 54-cell inner width; line: {controls:?}"
    );
    assert!(
        controls.ends_with('…'),
        "the query line must ellipsize with a marker; line: {controls:?}"
    );
    assert!(
        controls.starts_with("Query: "),
        "the ellipsized line must keep the Query label; line: {controls:?}"
    );
    Ok(())
}

/// A filter query short enough to fit the inner width renders verbatim with no
/// marker.
#[tokio::test]
async fn mcp_panel_controls_line_renders_short_query_verbatim() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(58, 20);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);
    for ch in "gh".chars() {
        driver
            .coordinator_mut()
            .state
            .status
            .mcp
            .append_mcp_panel_query_char(ch);
    }

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let controls = controls_row_text(&driver.buffer_text_in_rect(popup))?;
    assert_eq!(
        controls, "Query: gh",
        "a fitting query must render verbatim with no marker"
    );
    Ok(())
}

/// The model's controls line never exceeds the width it renders into across the
/// popup's whole width range.
#[test]
fn mcp_panel_controls_line_model_fits_every_inner_width() -> Result<()> {
    // -- Setup & Fixtures
    let state = AppState::default();

    // -- Exec & Check: inner widths 54..=70 are the popup 56..=72 range.
    for table_width in 54u16..=70 {
        let model = mcp_table_model_for_test(&state, table_width, 8);
        let width = UnicodeWidthStr::width(model.controls_line.as_str());
        assert!(
            width <= usize::from(table_width),
            "the controls line width {width} must not exceed inner width {table_width}; line: {:?}",
            model.controls_line
        );
    }
    Ok(())
}

/// The model's Query line never exceeds the width it renders into across the
/// popup's whole width range.
#[test]
fn mcp_panel_controls_line_model_fits_every_inner_width_with_query() -> Result<()> {
    // -- Setup & Fixtures: a query wider than every popup width allows.
    let mut state = AppState::default();
    for ch in "g".repeat(80).chars() {
        state.status.mcp.append_mcp_panel_query_char(ch);
    }

    // -- Exec & Check: inner widths 54..=70 are the popup 56..=72 range.
    for table_width in 54u16..=70 {
        let model = mcp_table_model_for_test(&state, table_width, 8);
        let width = UnicodeWidthStr::width(model.controls_line.as_str());
        assert!(
            width <= usize::from(table_width),
            "the query line width {width} must not exceed inner width {table_width}; line: {:?}",
            model.controls_line
        );
        assert!(
            model.controls_line.ends_with('…'),
            "the query line must carry the marker at inner width {table_width}; line: {:?}",
            model.controls_line
        );
    }
    Ok(())
}

// region:    --- Support

/// The first inner row of the popup as plain text, stripped of the frame border
/// columns and of the right padding left by a short line.
fn controls_row_text(popup_text: &str) -> Result<String> {
    let row = popup_text
        .lines()
        .nth(1)
        .ok_or("should render the controls row")?;
    let inner = row
        .chars()
        .skip(1)
        .take(row.chars().count().saturating_sub(2))
        .collect::<String>();
    Ok(inner.trim_end().to_string())
}

// endregion: --- Support
