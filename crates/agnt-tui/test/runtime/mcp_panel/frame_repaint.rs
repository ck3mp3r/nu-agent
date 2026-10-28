use super::*;

/// Moving the selection in the MCPs panel repaints the frame immediately: the
/// highlight symbol follows the key press without a reopen.
#[tokio::test]
async fn mcp_panel_selection_move_repaints_the_frame() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .status
        .mcp
        .set_mcp_servers(vec![
            crate::state::McpServerState {
                name: "gh".to_string(),
                state: McpServerUsabilityState::Enabled,
            },
            crate::state::McpServerState {
                name: "k8s".to_string(),
                state: McpServerUsabilityState::Disabled,
            },
        ]);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Mcps);
    driver.advance_with_frame(&[]).await?;

    // -- Check: the first frame highlights the first server.
    let first = highlighted_server_row(&driver).ok_or("the panel renders a highlighted row")?;
    assert!(first.contains("gh"), "the highlight starts on gh: {first}");

    // -- Exec: Down moves the selection, and the same pass repaints.
    driver.advance_with_frame(&[key(TerminalKey::Down)]).await?;

    // -- Check: the rendered frame follows the move.
    assert_eq!(driver.state().status.mcp.mcp_panel_selection, 1);
    let moved =
        highlighted_server_row(&driver).ok_or("the panel still renders a highlighted row")?;
    assert!(
        moved.contains("k8s"),
        "the highlight follows the move: {moved}"
    );
    Ok(())
}

/// The rendered row that carries the panel's highlight symbol and a server name.
fn highlighted_server_row(driver: &RenderLoopDriver) -> Option<String> {
    driver
        .buffer_text()
        .lines()
        .find(|line| line.contains('❯') && (line.contains("gh") || line.contains("k8s")))
        .map(str::to_string)
}
