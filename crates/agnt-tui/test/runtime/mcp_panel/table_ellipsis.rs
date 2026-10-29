use unicode_width::UnicodeWidthChar;

use super::*;

/// A server name wider than the Name column renders ellipsized inside the
/// popup at the minimum popup width.
#[tokio::test]
async fn mcp_table_ellipsis_clips_long_server_names() -> Result<()> {
    // -- Setup & Fixtures
    let long_name = format!("mcp-{}", "s".repeat(36));
    let mut driver = RenderLoopDriver::new(58, 20);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: long_name.clone(),
            state: McpServerUsabilityState::Enabled,
        }]);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: table width 54 leaves 30 chars for the Name column.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains('❯'))
        .ok_or("should render the selected server row")?;
    let clipped = format!("{}…", long_name.chars().take(29).collect::<String>());
    assert!(
        row.contains(&clipped),
        "the row should carry the ellipsized name {clipped:?}; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

/// The Name column absorbs the capped popup width: at the 72-column cap the
/// table is 70 wide, which leaves 46 chars for the Name column, so a 60-char
/// name renders ellipsized rather than whole.
#[tokio::test]
async fn mcp_table_ellipsis_name_column_absorbs_leftover_width() -> Result<()> {
    // -- Setup & Fixtures
    let long_name = format!("mcp-{}", "s".repeat(56));
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: long_name.clone(),
            state: McpServerUsabilityState::Enabled,
        }]);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the popup caps at 72; table width 70 leaves 46 chars for the
    // Name column.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    assert_eq!(
        popup.width, 72,
        "the MCP popup must cap at 72 columns at a 120-column terminal"
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains('❯'))
        .ok_or("should render the selected server row")?;
    let clipped = format!("{}…", long_name.chars().take(45).collect::<String>());
    assert!(
        row.contains(&clipped),
        "the row should carry the ellipsized name {clipped:?}; row: {row:?}\n{popup_text}"
    );
    assert!(
        !row.contains(&long_name),
        "the row should not render the full 60-char name; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

/// The 72-column cap leaves the full controls line untruncated.
#[tokio::test]
async fn mcp_table_ellipsis_controls_line_fits_capped_popup() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup inner width 70 holds the 60-char controls line whole.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let controls = mcp_panel_controls_line();
    assert_eq!(
        popup.width, 72,
        "the MCP popup must cap at 72 columns at a 120-column terminal"
    );
    assert!(
        popup_text.contains(controls),
        "the controls line {controls:?} should render untruncated\n{popup_text}"
    );
    Ok(())
}

/// A wide-glyph server name wider than the Name column truncates by display
/// cells, so the trailing ellipsis survives the column boundary.
#[tokio::test]
async fn mcp_table_ellipsis_cjk_server_name_keeps_ellipsis_in_budget() -> Result<()> {
    // -- Setup & Fixtures
    let name = "日本語".repeat(6);
    let head = name.chars().take(14).collect::<String>();
    let mut driver = RenderLoopDriver::new(58, 20);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: name.clone(),
            state: McpServerUsabilityState::Enabled,
        }]);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: table width 54 leaves 30 cells for the Name column, which holds
    // 14 two-cell glyphs plus the ellipsis.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let row = popup_text
        .lines()
        .find(|line| line.contains('❯'))
        .ok_or("should render the selected server row")?;
    let row_text = compact_wide_cells(row);
    let clipped = format!("{head}…");
    assert!(
        row_text.contains(&clipped),
        "the row should carry the cell-budgeted name {clipped:?}; row: {row:?}\n{popup_text}"
    );
    assert!(
        !row_text.contains(&name),
        "the row should not render the full 36-cell name; row: {row:?}\n{popup_text}"
    );
    Ok(())
}

// region:    --- Support

/// The test backend writes a two-cell glyph in its first cell and leaves the
/// continuation cell as a space, so one buffer row spells `日本語` as
/// `日 本 語`. Drop the continuation cell to compare against the logical text.
fn compact_wide_cells(text: &str) -> String {
    let mut out = String::new();
    let mut continuation = false;
    for ch in text.chars() {
        if continuation && ch == ' ' {
            continuation = false;
            continue;
        }
        continuation = UnicodeWidthChar::width(ch).unwrap_or(0) == 2;
        out.push(ch);
    }
    out
}

// endregion: --- Support
