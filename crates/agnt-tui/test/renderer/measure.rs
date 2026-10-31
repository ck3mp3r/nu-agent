//! Measure/paint agreement: the backend's `measure` is the height index's only
//! source, so a measured row the widget never paints (or vice versa) desyncs the
//! scroll math.

use super::support::*;
use agnt_core::transcript::ir::MessageRole;
use agnt_core::transcript::renderer::ItemStatus;

/// WHEN a short markdown entry is measured and painted, THE painted row count
/// SHALL equal the measured one.
#[test]
fn test_renderer_measure_markdown_matches_painted_rows() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, "short line");

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1, "a short line is one row");
    assert_eq!(painted_rows(&backend), rows);
    Ok(())
}

/// WHEN wrapping prose is measured and painted, THE painted row count SHALL
/// equal the measured one.
#[test]
fn test_renderer_measure_wrapping_prose_matches_painted_rows() -> Result<()> {
    // -- Setup & Fixtures
    let entry = message_entry(MessageRole::User, &long_prose());

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert!(rows >= 2, "the prose must wrap; got {rows}");
    assert_eq!(painted_rows(&backend), rows);
    Ok(())
}

/// WHEN a code preview is measured and painted, THE painted row count SHALL
/// equal the measured one and SHALL include the surface's two margin rows.
#[test]
fn test_renderer_measure_code_preview_includes_surface_margins() -> Result<()> {
    // -- Setup & Fixtures: one call row and a three-line code body.
    let entry = tool_entry(
        "nu",
        "→ ls",
        Some(code_display("nu", "alpha\nbeta\ngamma")),
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: call + two margins + three code rows.
    assert_eq!(rows, 6, "the surface contributes its margins and rows");
    assert_eq!(painted_rows(&backend), rows);
    let surface = terminal(test_theme().code_surface);
    assert_eq!(
        cell_bg(&backend, 0, 1),
        Some(surface),
        "the top margin paints the surface"
    );
    assert_eq!(
        cell_bg(&backend, 0, u16::try_from(rows)? - 1),
        Some(surface),
        "the bottom margin paints the surface"
    );
    Ok(())
}

/// WHEN a diff preview is measured and painted, THE painted row count SHALL
/// equal the measured one.
#[test]
fn test_renderer_measure_diff_preview_matches_painted_rows() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        "edit",
        "→ a.rs",
        Some(diff_display(SIMPLE_DIFF)),
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert!(rows >= 6, "the diff body must render; got {rows}");
    assert_eq!(painted_rows(&backend), rows);
    Ok(())
}

/// WHEN the call line carries a status indicator, THE measured row count SHALL
/// equal the painted one — the indicator shrinks row 0's wrap budget, so both
/// sides must wrap identically.
#[test]
fn test_renderer_measure_status_indicator_matches_painted_rows() -> Result<()> {
    // -- Setup & Fixtures: a summary far wider than the pane.
    let entry = tool_entry(
        "read",
        &format!("\u{2192} {}", "x".repeat(200)),
        None,
        None,
        ItemStatus::InProgress,
    );

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert!(rows >= 2, "the summary must wrap; got {rows}");
    assert_eq!(painted_rows(&backend), rows);
    Ok(())
}

/// WHEN a plain display is measured and painted, THE painted row count SHALL
/// equal the measured one.
#[test]
fn test_renderer_measure_plain_display_matches_painted_rows() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        "read",
        "→ a.rs",
        Some(plain_display("one plain body line")),
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: call row plus one body row, no surface.
    assert_eq!(rows, 2);
    assert_eq!(painted_rows(&backend), rows);
    Ok(())
}
