use super::*;

use agnt_runtime::tools::mcp::runtime::McpServerLifecycle;

/// What the log carries and the panel shows collapsed: a child's spawn dump,
/// one line at storage.
const DUMP: &str = "failed to connect stdio MCP server: process exited with status 2 usage: stub-mcp [--flag] error: unexpected argument '--injected' found";

/// A lifecycle projection carrying a connect failure reason.
fn failed_lifecycle(name: &str, reason: Option<&str>) -> McpServerLifecycle {
    McpServerLifecycle {
        name: name.to_string(),
        configured: true,
        enabled: true,
        connected: false,
        visible_tool_count: 0,
        failure_reason: reason.map(str::to_string),
    }
}

/// WHEN the projection carries a server's failure reason, THE surface SHALL hold
/// it for that server, and the details SHALL read it instead of `Error: None`.
#[test]
fn mcp_lifecycle_failure_reason_reaches_the_details() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec
    coordinator.set_mcp_lifecycle_projection(vec![
        failed_lifecycle("k8s", Some(DUMP)),
        failed_lifecycle("gh", None),
    ]);

    // -- Check: the failed server carries its reason into the details.
    coordinator.state.status.mcp.mcp_panel_selection = 0;
    let details = super::mcp_selected_details_for_test(&coordinator.state)
        .ok_or("should have selected MCP details")?;
    assert_eq!(details.server_line, "Server: k8s (failed)");
    assert_eq!(details.error_line, format!("Error: {DUMP}"));

    // -- Check: a failed server with no recorded reason keeps `Error: None`.
    coordinator.state.status.mcp.mcp_panel_selection = 1;
    let bare = super::mcp_selected_details_for_test(&coordinator.state)
        .ok_or("should have selected MCP details")?;
    assert_eq!(bare.error_line, "Error: None");
    Ok(())
}

/// WHEN the reason does not fit the details rows, THE pane SHALL show the
/// reason's head and end on the marker row that points at the log.
#[test]
fn mcp_details_cut_reason_ends_on_the_marker_row() -> Result<()> {
    // -- Setup & Fixtures
    const HEIGHT: u16 = 3;
    const WIDTH: u16 = 60;
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_mcp_lifecycle_projection(vec![failed_lifecycle("k8s", Some(DUMP))]);

    // -- Exec
    let rows = super::mcp_painted_details_rows_for_test(&coordinator.state, HEIGHT, WIDTH);

    // -- Check: the head of the reason is visible and the last row is the
    // marker, worded exactly.
    assert_eq!(rows.len(), usize::from(HEIGHT), "rows: {rows:?}");
    assert_eq!(rows[0], "Server: k8s (failed)");
    assert!(
        rows[1].starts_with("Error: failed to connect stdio MCP server:"),
        "the reason's head must show, got: {:?}",
        rows[1]
    );
    assert_eq!(rows[2], "… full reason in the log");

    // -- Check: every row fits the pane and carries no control byte.
    for row in &rows {
        assert!(
            row.chars().count() <= usize::from(WIDTH),
            "row over {WIDTH} cells: {row:?}"
        );
        assert!(
            !row.chars().any(char::is_control),
            "row carries a control byte: {row:?}"
        );
    }
    Ok(())
}

/// WHEN the reason fits the details rows, THE pane SHALL render it whole with
/// no marker:
#[test]
fn mcp_details_fitting_reason_has_no_marker() -> Result<()> {
    // -- Setup & Fixtures
    let reason = "initialize handshake did not complete within 300ms";
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_mcp_lifecycle_projection(vec![failed_lifecycle("k8s", Some(reason))]);

    // -- Exec
    let rows = super::mcp_painted_details_rows_for_test(&coordinator.state, 6, 80);

    // -- Check
    assert_eq!(
        rows,
        vec![
            "Server: k8s (failed)".to_string(),
            format!("Error: {reason}"),
            "Tools: None".to_string(),
        ]
    );
    Ok(())
}

/// WHEN the details height collapses to one row, THE pane SHALL show the single
/// collapsed line while it fits, and the marker once the collapse drops content.
#[test]
fn mcp_details_single_row_marker_only_when_collapsed() -> Result<()> {
    // -- Setup & Fixtures
    let short = "initialize handshake did not complete within 300ms";
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_mcp_lifecycle_projection(vec![failed_lifecycle("k8s", Some(short))]);

    // -- Exec
    let fitting = super::mcp_painted_details_rows_for_test(&coordinator.state, 1, 80);

    // -- Check: the fitting collapse renders whole.
    assert_eq!(
        fitting,
        vec![format!("Error: {short} · Server: k8s (failed)")]
    );

    // -- Setup & Fixtures: the same server, a reason past the row.
    let mut narrow = RuntimeCoordinator::new(120, 40, Some(false));
    narrow.set_mcp_lifecycle_projection(vec![failed_lifecycle("k8s", Some(DUMP))]);

    // -- Exec
    let collapsed = super::mcp_painted_details_rows_for_test(&narrow.state, 1, 80);

    // -- Check: the dropped content is marked.
    assert_eq!(collapsed, vec!["… full reason in the log".to_string()]);
    Ok(())
}
