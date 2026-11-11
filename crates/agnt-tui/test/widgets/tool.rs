use super::support::*;
use agnt_render::transcript::renderer::ItemStatus;
use agnt_runtime::protocol::contracts::UiMessageSnapshot;
use agnt_runtime::session::tool_status::ToolStatus;

use crate::interaction::reducer::apply_permission_request_display;
use crate::runtime::RuntimeCoordinator;
use crate::state::AppState;
use crate::test_support::{fold_record, tool_completed, tool_started};

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
// The completed edit keeps the gate preview's band
// ---------------------------------------------------------------------------

/// WHEN one live edit is asked at the gate and then accepted, THE completed
/// entry SHALL paint its diff rows with the backgrounds the gate preview
/// painted: the file-header and hunk rows keep the code surface, and the
/// changed rows keep their tints. The band is the body's content, not the
/// entry's slot.
#[test]
fn test_widgets_tool_completed_edit_keeps_the_gate_preview_backgrounds() -> Result<()> {
    // -- Setup & Fixtures: one live call, asked at the gate with the diff the
    // producer computed.
    let mut state = AppState::default();
    fold_record(&mut state, tool_started(edit_call()));
    apply_permission_request_display(&mut state, &edit_gate(EDIT_DIFF));

    // -- Exec: capture the gate preview's rows, then fold the completion with
    // the edit handler's own payload and capture the completed rows.
    let preview_entry = only_entry(&state)?.clone();
    let (preview, preview_rows) = render_at(&preview_entry, 0)?;
    fold_record(
        &mut state,
        tool_completed(edit_call(), edit_outcome(EDIT_DIFF)),
    );
    let (completed, completed_rows) = render_at(only_entry(&state)?, 0)?;

    // -- Check: the headers and the hunk carry the code surface in both
    // captures; the changed rows carry their tints in both.
    let preview_backgrounds = edit_capture_backgrounds(&preview, preview_rows)?;
    let completed_backgrounds = edit_capture_backgrounds(&completed, completed_rows)?;
    assert_eq!(
        completed_backgrounds, preview_backgrounds,
        "the completed edit paints the gate preview's backgrounds row for row"
    );
    let expected: Vec<_> = edit_row_backgrounds()
        .iter()
        .map(|(_, background)| *background)
        .collect();
    assert_eq!(preview_backgrounds, expected, "the preview band");
    assert_eq!(completed_backgrounds, expected, "the completed band");

    // -- Check: the completion's own stats row rides the same band.
    assert_eq!(
        row_background(&completed, completed_rows, "files=1")?,
        Some(terminal(test_theme().code_surface)),
        "the stats row sits on the code surface"
    );
    Ok(())
}

/// WHEN a resumed session's edit row is rehydrated from the persisted pair, THE
/// replayed row SHALL paint the backgrounds the live completion painted: the
/// band derives from the body's content, so the replay lane inherits it.
#[test]
fn test_widgets_tool_rehydrated_edit_paints_the_live_backgrounds() -> Result<()> {
    // -- Setup & Fixtures: the live lane, asked at the gate and completed.
    let mut state = AppState::default();
    fold_record(&mut state, tool_started(edit_call()));
    apply_permission_request_display(&mut state, &edit_gate(EDIT_DIFF));
    fold_record(
        &mut state,
        tool_completed(edit_call(), edit_outcome(EDIT_DIFF)),
    );
    let (live, live_rows) = render_at(only_entry(&state)?, 0)?;

    // -- Exec: the persisted pair — the call's arguments and the handler's own
    // result text — rehydrated through the replay fold.
    let snapshot = UiMessageSnapshot::new("tool", "")
        .with_tool_name("edit".to_string())
        .with_tool_details(
            Some(edit_call().arguments.clone()),
            Some(edit_result_text(EDIT_DIFF)),
            Some(true),
        )
        .with_tool_status(ToolStatus::Success);
    let mut coordinator = RuntimeCoordinator::new(120, HEIGHT, Some(true));
    coordinator.hydrate_transcript_from_messages(vec![snapshot], None);
    let (replayed, replayed_rows) = render_at(only_entry(coordinator.state())?, 0)?;

    // -- Check: the replayed row paints the live capture's backgrounds row for
    // row, and both carry the expectation table's band.
    let live_backgrounds = edit_capture_backgrounds(&live, live_rows)?;
    let replayed_backgrounds = edit_capture_backgrounds(&replayed, replayed_rows)?;
    assert_eq!(
        replayed_backgrounds, live_backgrounds,
        "the rehydrated edit paints the live completion's backgrounds row for row"
    );
    let expected: Vec<_> = edit_row_backgrounds()
        .iter()
        .map(|(_, background)| *background)
        .collect();
    assert_eq!(replayed_backgrounds, expected, "the rehydrated band");
    Ok(())
}

// ---------------------------------------------------------------------------
// Display selection and plain bodies
// ---------------------------------------------------------------------------

/// WHEN the entry carries both a gate preview and a completion result, THE
/// widget SHALL render the result alone — the result takes over the entry's
/// unified display slot — on the code surface its own content names.
#[test]
fn test_widgets_tool_result_takes_over_the_unified_display_slot() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        Some(plain_display("gate preview")),
        Some(code_display("nu", "result body")),
        ItemStatus::Done,
    );
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: the result body renders; the preview is gone.
    let text: String = (0..rows as u16).map(|y| row_text(&backend, y)).collect();
    assert!(text.contains("result body"), "the result renders");
    assert!(
        !text.contains("gate preview"),
        "the result takes over: the preview must not render beside it"
    );

    // -- Check: the completion body is a code body, so it keeps the band.
    assert_eq!(rows, 4, "call + two margins + one body row");
    assert!(
        is_untinted(cell_bg(&backend, 0, 0)),
        "the call line stays untinted"
    );
    for y in 1..u16::try_from(rows)? {
        assert_eq!(
            cell_bg(&backend, 0, y),
            Some(terminal(theme.code_surface)),
            "row {y} paints the code surface"
        );
    }
    Ok(())
}

/// WHEN a completion display carries a diff, THE diff rows SHALL keep their
/// tints over the code surface: the band is the body's content, so the
/// completion paints exactly what the gate preview painted.
#[test]
fn test_widgets_tool_completion_diff_keeps_the_band() -> Result<()> {
    // -- Setup & Fixtures
    let entry = tool_entry(
        None,
        Some(diff_display("@@ -1 +1 @@\n-old\n+new\n")),
        ItemStatus::Done,
    );
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: call, top margin, hunk, removed, added, bottom margin.
    assert_eq!(rows, 6, "call + two margins + three diff rows");
    assert!(is_untinted(cell_bg(&backend, 0, 0)));
    assert_eq!(
        cell_bg(&backend, 0, 1),
        Some(terminal(theme.code_surface)),
        "the top margin paints the code surface"
    );
    assert_eq!(
        cell_bg(&backend, 0, 2),
        Some(terminal(theme.code_surface)),
        "the hunk row sits on the code surface"
    );
    assert_eq!(
        cell_bg(&backend, 0, 3),
        Some(terminal(theme.diff_remove_bg)),
        "the removed row keeps its tint"
    );
    assert_eq!(
        cell_bg(&backend, 0, 4),
        Some(terminal(theme.diff_add_bg)),
        "the added row keeps its tint"
    );
    assert_eq!(
        cell_bg(&backend, 0, 5),
        Some(terminal(theme.code_surface)),
        "the bottom margin closes the band"
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
