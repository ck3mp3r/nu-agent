use super::*;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Render one frame for a coordinator built with `side_pane_visible`, and return
/// the screen as plain text.
fn render_with_side_pane(
    side_pane_visible: Option<bool>,
    columns: u16,
    rows: u16,
) -> Result<String> {
    let mut coordinator = RuntimeCoordinator::new(columns, rows, side_pane_visible);
    let mut terminal = Terminal::new(TestBackend::new(columns, rows))?;
    let mut live = Some(&mut terminal);
    coordinator.render_frame(&mut live)?;

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    Ok(text)
}

/// WHEN the coordinator is constructed with the side pane requested, THE frame
/// SHALL carry the pane; WHEN it is declined or left unset, THE frame SHALL NOT.
/// The constructor stores the argument, so the pane's visibility follows it.
#[test]
fn test_runtime_coordinator_honors_side_pane_visible() -> Result<()> {
    // -- Setup & Fixtures
    const COLUMNS: u16 = 120;
    const ROWS: u16 = 30;

    // -- Exec
    let requested = render_with_side_pane(Some(true), COLUMNS, ROWS)?;
    let unset = render_with_side_pane(None, COLUMNS, ROWS)?;
    let declined = render_with_side_pane(Some(false), COLUMNS, ROWS)?;

    // -- Check
    assert!(
        requested.contains("Events"),
        "a requested side pane must render; frame:\n{requested}"
    );
    assert!(
        !unset.contains("Events"),
        "an unset side pane must not render; frame:\n{unset}"
    );
    assert!(
        !declined.contains("Events"),
        "a declined side pane must not render; frame:\n{declined}"
    );
    Ok(())
}
