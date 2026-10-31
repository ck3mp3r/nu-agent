//! The code surface: margins, the untinted call line, and the surface body.

use super::support::*;
use agnt_core::transcript::renderer::ItemStatus;

/// The code preview under test: a titled, labelled nu body.
fn code_preview() -> agnt_core::transcript::entry::TranscriptEntry {
    tool_entry(
        "nu",
        "→ ls | select name",
        Some(titled_code_display("nu", "ls | select name")),
        None,
        ItemStatus::Done,
    )
}

/// WHEN a code preview renders, THE top margin row of the band SHALL paint the
/// code surface, directly below the untinted call line.
#[test]
fn test_renderer_fill_code_top_margin_paints_the_surface() -> Result<()> {
    // -- Setup & Fixtures
    let entry = code_preview();
    let surface = terminal(test_theme().code_surface);

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert!(rows > 3, "the surface needs margins and rows; got {rows}");
    for x in [0u16, 1, WIDTH - 1] {
        assert_eq!(
            cell_bg(&backend, x, 1),
            Some(surface),
            "the top margin paints the surface at column {x}"
        );
    }
    Ok(())
}

/// WHEN a code preview renders, THE bottom margin row SHALL paint the code
/// surface across the pane.
#[test]
fn test_renderer_fill_code_bottom_margin_paints_the_surface() -> Result<()> {
    // -- Setup & Fixtures
    let entry = code_preview();
    let surface = terminal(test_theme().code_surface);

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    let bottom = u16::try_from(rows)? - 1;
    for x in [0u16, 1, WIDTH - 1] {
        assert_eq!(
            cell_bg(&backend, x, bottom),
            Some(surface),
            "the bottom margin paints the surface at column {x}"
        );
    }
    Ok(())
}

/// WHEN a code preview renders, THE call line SHALL stay untinted — it opens
/// the entry, and the surface band starts on the margin row below it.
#[test]
fn test_renderer_fill_code_call_line_is_untinted() -> Result<()> {
    // -- Setup & Fixtures
    let entry = code_preview();
    let surface = terminal(test_theme().code_surface);

    // -- Exec
    let (backend, _) = render_at(&entry, 0)?;

    // -- Check
    let row = row_trimmed(&backend, 0);
    assert!(
        row.contains('\u{2713}'),
        "row 0 carries the status glyph; got {row:?}"
    );
    for x in 0..WIDTH {
        assert_ne!(
            cell_bg(&backend, x, 0),
            Some(surface),
            "the call line must not be tinted at column {x}"
        );
    }
    Ok(())
}

/// WHEN a code preview renders, EVERY row of the band SHALL paint the code
/// surface: the title, the label, the body and the wrapped rows all sit on one
/// surface, framed by the margin rows.
#[test]
fn test_renderer_fill_code_inner_rows_paint_the_surface() -> Result<()> {
    // -- Setup & Fixtures
    let entry = code_preview();
    let surface = terminal(test_theme().code_surface);

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: rows 1..rows-1 are the surface, margins included.
    assert!(rows > 3, "the fixture must render a body; got {rows}");
    for y in 1..u16::try_from(rows)? - 1 {
        assert_eq!(
            cell_bg(&backend, 0, y),
            Some(surface),
            "row {y} must paint the surface at column 0"
        );
        assert_eq!(
            cell_bg(&backend, WIDTH - 1, y),
            Some(surface),
            "row {y} must paint the surface at the pane edge"
        );
    }
    Ok(())
}

/// WHEN a tool's preview is a diff, THE widget SHALL paint the call line and the
/// diff body with its line-number gutter.
#[test]
fn test_renderer_fill_code_diff_preview_paints_gutter_lines() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        "edit",
        "→ a.rs (diff)",
        Some(diff_display(SIMPLE_DIFF)),
        None,
        ItemStatus::Done,
    );

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    let text: String = (0..u16::try_from(rows)?)
        .map(|y| row_text(&backend, y))
        .collect();
    assert!(
        text.contains("→ a.rs (diff)"),
        "row 0 must carry the call summary; got {text:?}"
    );
    assert!(
        text.contains("\u{2502}new") && text.contains("\u{2502}old"),
        "the diff body must keep its line-number gutter; got {text:?}"
    );
    Ok(())
}
