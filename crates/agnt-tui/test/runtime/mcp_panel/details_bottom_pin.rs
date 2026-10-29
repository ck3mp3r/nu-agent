use super::*;

/// A failed server whose error text wraps keeps every details row: the pane is
/// sized to the rendered rows (four), so the tool line still renders on the
/// bottom inner row instead of being pushed past the rect.
#[tokio::test]
async fn mcp_details_pane_sizes_to_a_wrapped_error_line() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(62, 24);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_server_state_by_name_with_reason(
            "k8s",
            McpServerUsabilityState::Failed,
            Some(
                "connection timeout while dialing 10.0.0.1:443 after many retries and additional context"
                    .to_string(),
            ),
        );
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the 89-cell error wraps to two rows at width 58, so the pane is
    // four rows and its last row carries the tool line.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[15].contains("Server: k8s (failed)"),
        "the server line must open the four-row details at row 15; row: {:?}\n{popup_text}",
        lines[15]
    );
    assert!(
        lines[16].contains("Error: connection timeout while dialing"),
        "the error must start on row 16; row: {:?}\n{popup_text}",
        lines[16]
    );
    assert!(
        lines[17].contains("many retries and additional context"),
        "the wrapped error tail must render on row 17; row: {:?}\n{popup_text}",
        lines[17]
    );
    assert!(
        lines[18].contains("Tools: None"),
        "the tool line must render on the bottom inner row (18); row: {:?}\n{popup_text}",
        lines[18]
    );
    Ok(())
}

/// At 60x20 the popup inner height is 18, so the details ladder caps the pane
/// at six rows while a one-tool server needs three. The details occupy the
/// bottom three inner rows, the last one directly above the bottom border, with
/// no reserved blank band below the text.
#[tokio::test]
async fn mcp_details_pane_pins_to_the_bottom_edge_at_popup_height_twenty() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(62, 24);
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
        .status
        .mcp
        .set_mcp_visible_tool_names_by_server_name("gh", vec!["gh__issue_list".to_string()]);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 60x20, inner 58x18, ladder cap 6, details 3 rows. The
    // details rows are popup rows 16-18; row 18 is the last row before the
    // border at 19.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[16].contains("Server: gh (enabled)"),
        "the server line must open the pinned details at row 16; row: {:?}\n{popup_text}",
        lines[16]
    );
    assert!(
        lines[17].contains("Error:"),
        "the error line must render on row 17; row: {:?}\n{popup_text}",
        lines[17]
    );
    assert!(
        lines[18].contains("Tools: gh__issue_list"),
        "the last details line must sit on the bottom inner row (18); row: {:?}\n{popup_text}",
        lines[18]
    );
    assert!(
        !lines[15].contains("Server:"),
        "no details text may sit above the pinned rows; row: {:?}\n{popup_text}",
        lines[15]
    );
    Ok(())
}

/// At 60x20 a one-tool server needs three details rows out of the six the
/// ladder caps, so the table area takes the freed rows: eleven servers fit
/// without an overflow cue instead of being windowed to nine.
#[tokio::test]
async fn mcp_details_pane_frees_rows_for_the_table_when_details_underfill() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(62, 24);
    driver.coordinator_mut().state.status.mcp.set_mcp_servers(
        (0..11)
            .map(|idx| crate::state::McpServerState {
                name: format!("srv-{idx}"),
                state: McpServerUsabilityState::Enabled,
            })
            .collect(),
    );
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the table absorbs every row above the gap row and the three
    // details rows, so all eleven servers render and the title carries no
    // overflow cue.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[0].contains("MCPs") && !lines[0].contains('/'),
        "the title must carry no overflow cue when every row fits; title: {:?}\n{popup_text}",
        lines[0]
    );
    assert!(
        popup_text.contains("srv-10"),
        "the last server row must render above the details; popup:\n{popup_text}"
    );
    assert!(
        lines[18].contains("Tools:"),
        "the last details line must sit on the bottom inner row (18); row: {:?}\n{popup_text}",
        lines[18]
    );
    Ok(())
}

/// The details pane claims one gap row above its first line, so the table area
/// ends one row short of the details: a twelfth server is windowed away and the
/// row directly above the details stays blank base background.
#[tokio::test]
async fn mcp_details_pane_leaves_a_gap_row_above_the_details() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::default();
    let mut driver = RenderLoopDriver::new(62, 24);
    driver.coordinator_mut().state.status.mcp.set_mcp_servers(
        (0..12)
            .map(|idx| crate::state::McpServerState {
                name: format!("srv-{idx}"),
                state: McpServerUsabilityState::Enabled,
            })
            .collect(),
    );
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 60x20 → inner 58x18 → the three details rows sit at popup
    // rows 16-18, the gap takes row 15, and the table's eleven body rows end
    // above it.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[15].trim_matches(['│', ' ']).is_empty(),
        "the gap row above the details must carry no text; row: {:?}\n{popup_text}",
        lines[15]
    );
    assert_eq!(
        driver.cell_style_at(popup.x + 5, 15).bg,
        Some(theme.base),
        "the gap row must keep the base background; popup:\n{popup_text}"
    );
    assert!(
        popup_text.contains("srv-10") && !popup_text.contains("srv-11"),
        "the table must window the servers the gap row pushes out; popup:\n{popup_text}"
    );
    assert!(
        lines[16].contains("Server: srv-0 (enabled)"),
        "the details must open on row 16; row: {:?}\n{popup_text}",
        lines[16]
    );
    assert!(
        lines[18].contains("Tools:"),
        "the last details line must sit on the bottom inner row (18); row: {:?}\n{popup_text}",
        lines[18]
    );
    Ok(())
}

/// With no server details the pane renders nothing, so the table takes the full
/// inner area below the controls row.
#[tokio::test]
async fn mcp_details_pane_renders_nothing_without_a_selected_server() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(62, 24);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: no details text and no blank reserved band claims the pane.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[1].contains("↑/↓ select"),
        "the controls line must render on the first content row; row: {:?}\n{popup_text}",
        lines[1]
    );
    assert!(
        !popup_text.contains("Server:") && !popup_text.contains("Error:"),
        "no details text may render without a selected server; popup:\n{popup_text}"
    );
    Ok(())
}

/// Details longer than the ladder cap stay bounded: the pane fills the six cap
/// rows and the tool tail collapses into a "+N more" cue on the bottom row.
#[tokio::test]
async fn mcp_details_pane_fills_the_ladder_cap_and_trims_the_tail() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(62, 24);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    let tool_names: Vec<String> = (0..10)
        .map(|idx| format!("gh__tool_{:0>30}", idx))
        .collect();
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_visible_tool_names_by_server_name("gh", tool_names);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: popup 60x20 → ladder cap 6, so the details fill popup rows 13-18
    // and the dropped tools are counted on the bottom row.
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Mcps,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    assert!(
        lines[13].contains("Server: gh (enabled)"),
        "the capped pane must open on row 13; row: {:?}\n{popup_text}",
        lines[13]
    );
    assert!(
        lines[18].contains("more"),
        "the trimmed tool tail must show the \"+N more\" cue on row 18; row: {:?}\n{popup_text}",
        lines[18]
    );
    assert!(
        !lines[12].contains("Server:"),
        "the capped pane must not grow past its cap; row: {:?}\n{popup_text}",
        lines[12]
    );
    Ok(())
}
