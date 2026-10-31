use super::support::*;
use agnt_core::transcript::renderer::ItemStatus;

// ---------------------------------------------------------------------------
// Call line
// ---------------------------------------------------------------------------

/// WHEN the tool widget renders a bare call, THE call line SHALL carry the
/// lane prefix, the status glyph, the name in text and the summary muted — on
/// no surface at all.
#[test]
fn test_widgets_tool_call_line_is_untinted() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(None, None, ItemStatus::Done);
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1, "a bare call is one row");
    assert_eq!(row_text(&backend, 0).trim_end(), "  ⚙ ✓ read → a.rs");
    assert!(
        is_untinted(cell_bg(&backend, 0, 0)),
        "the call line is untinted"
    );
    assert!(is_untinted(cell_bg(&backend, WIDTH - 1, 0)));
    assert_eq!(cell_fg(&backend, 6, 0), Some(terminal(theme.text)));
    assert_eq!(cell_fg(&backend, 11, 0), Some(terminal(theme.muted)));
    Ok(())
}

// ---------------------------------------------------------------------------
// Code surface
// ---------------------------------------------------------------------------

/// WHEN the tool widget renders a code preview, THE call line SHALL open the
/// entry untinted and the surface band SHALL sit below it, closing with its own
/// top and bottom margin rows.
#[test]
fn test_widgets_tool_code_preview_paints_surface_with_margins() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        Some(code_display("nu", "ls | select name")),
        None,
        ItemStatus::Done,
    );
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: the call row, then the band's margins around the code row.
    assert_eq!(rows, 4, "call + two margins + one code row");
    assert!(
        is_untinted(cell_bg(&backend, 0, 0)),
        "the call line is untinted"
    );
    for y in [1u16, 2, 3] {
        assert_eq!(
            cell_bg(&backend, 0, y),
            Some(terminal(theme.code_surface)),
            "row {y} paints the code surface"
        );
        assert_eq!(
            cell_bg(&backend, WIDTH - 1, y),
            Some(terminal(theme.code_surface)),
            "row {y} paints the surface to the pane edge"
        );
    }
    assert!(
        row_text(&backend, 2).contains("ls | select name"),
        "the code body renders: {:?}",
        row_text(&backend, 2)
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Diff surface
// ---------------------------------------------------------------------------

/// WHEN the tool widget renders a diff preview, THE added and removed rows
/// SHALL paint their diff tints over the code surface, and the numbers SHALL
/// keep their gutter.
#[test]
fn test_widgets_tool_diff_preview_paints_line_tints() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        Some(diff_display("@@ -1 +1 @@\n-old\n+new\n")),
        None,
        ItemStatus::Done,
    );
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: call, top margin, hunk, removed, added, bottom margin.
    assert_eq!(rows, 6, "call + two margins + three diff rows");
    assert!(
        is_untinted(cell_bg(&backend, 0, 0)),
        "the call line stays untinted"
    );
    assert_eq!(cell_bg(&backend, 0, 2), Some(terminal(theme.code_surface)));
    assert_eq!(
        cell_bg(&backend, 0, 3),
        Some(terminal(theme.diff_remove_bg))
    );
    assert_eq!(cell_bg(&backend, 0, 4), Some(terminal(theme.diff_add_bg)));
    assert_eq!(cell_bg(&backend, 0, 5), Some(terminal(theme.code_surface)));
    assert_eq!(
        cell_bg(&backend, WIDTH - 1, 4),
        Some(terminal(theme.diff_add_bg)),
        "the tint covers the whole row"
    );
    assert!(
        row_text(&backend, 3).contains("│old"),
        "the removed row keeps its line-number gutter: {:?}",
        row_text(&backend, 3)
    );
    assert!(
        row_text(&backend, 4).contains("│new"),
        "the added row keeps its line-number gutter: {:?}",
        row_text(&backend, 4)
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Display selection and plain bodies
// ---------------------------------------------------------------------------

/// WHEN the entry carries both a gate preview and a completion result, THE
/// widget SHALL render the result alone — the result takes over the entry's
/// unified display slot — and the completion SHALL render bare: the pre-wave
/// pane gave the code surface to the gate preview only.
#[test]
fn test_widgets_tool_result_takes_over_the_unified_display_slot() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        Some(plain_display("gate preview")),
        Some(code_display("nu", "result body")),
        ItemStatus::Done,
    );

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: the result body renders; the preview is gone.
    let text: String = (0..rows as u16).map(|y| row_text(&backend, y)).collect();
    assert!(text.contains("result body"), "the result renders");
    assert!(
        !text.contains("gate preview"),
        "the result takes over: the preview must not render beside it"
    );

    // -- Check: the completion is bare — no surface, no margin rows.
    assert_eq!(rows, 2, "call + one body row: no band around a completion");
    for y in 0..u16::try_from(rows)? {
        assert!(
            is_untinted(cell_bg(&backend, 0, y)),
            "row {y} must stay untinted on a completion display"
        );
    }
    Ok(())
}

/// WHEN a completion display carries a diff, THE diff rows SHALL keep their
/// line tints on the pane — bare rows, no surface band and no margins.
#[test]
fn test_widgets_tool_completion_diff_paints_tints_without_a_band() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        None,
        Some(diff_display("@@ -1 +1 @@\n-old\n+new\n")),
        ItemStatus::Done,
    );
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: call, hunk, removed, added — no margins.
    assert_eq!(rows, 4, "call + three diff rows, no band");
    assert!(is_untinted(cell_bg(&backend, 0, 0)));
    assert!(
        is_untinted(cell_bg(&backend, 0, 1)),
        "the hunk row sits on the pane"
    );
    assert_eq!(
        cell_bg(&backend, 0, 2),
        Some(terminal(theme.diff_remove_bg)),
        "the removed row keeps its tint on the pane"
    );
    assert_eq!(
        cell_bg(&backend, 0, 3),
        Some(terminal(theme.diff_add_bg)),
        "the added row keeps its tint on the pane"
    );
    Ok(())
}

/// WHEN the display carries no code or diff content, THE body rows SHALL stay
/// untinted and keep the continuation gutter.
#[test]
fn test_widgets_tool_plain_display_has_no_surface() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        Some(plain_display("no surface here")),
        None,
        ItemStatus::Done,
    );
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 2, "call + one plain row");
    assert!(is_untinted(cell_bg(&backend, 0, 1)));
    assert!(row_text(&backend, 1).starts_with("    no surface here"));
    assert_eq!(cell_fg(&backend, 4, 1), Some(terminal(theme.text)));
    Ok(())
}

/// WHEN a plain display carries stats, THE stats row SHALL render muted above
/// the body.
#[test]
fn test_widgets_tool_plain_display_renders_stats_row() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(Some(stat_display()), None, ItemStatus::Done);
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: call, stats row, body row.
    assert_eq!(rows, 3);
    assert!(row_text(&backend, 1).contains("files=1 +2 -0"));
    assert_eq!(cell_fg(&backend, 4, 1), Some(terminal(theme.muted)));
    assert!(row_text(&backend, 2).contains("body"));
    Ok(())
}
