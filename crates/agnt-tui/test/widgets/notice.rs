use super::support::*;
use agnt_core::transcript::ir::NoticeKind;

// ---------------------------------------------------------------------------
// System notices
// ---------------------------------------------------------------------------

/// WHEN the notice widget renders a system notice, THE row SHALL carry the
/// notice lane prefix and the notice foreground with no surface.
#[test]
fn test_widgets_notice_system_lane_and_color() -> Result<()> {
    // -- Setup & Fixtures
    let entry = notice_entry(NoticeKind::System, "All done");
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1);
    assert_eq!(row_text(&backend, 0).trim_end(), "  · All done");
    assert!(is_untinted(cell_bg(&backend, 0, 0)));
    assert_eq!(cell_fg(&backend, 4, 0), Some(terminal(theme.notice)));
    Ok(())
}

/// WHEN the notice widget renders a compaction notice, THE row SHALL carry the
/// compaction marker and the muted foreground.
#[test]
fn test_widgets_notice_compaction_lane_and_color() -> Result<()> {
    // -- Setup & Fixtures
    let entry = notice_entry(NoticeKind::Compaction, "Compacting");
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check
    assert_eq!(rows, 1);
    assert_eq!(row_text(&backend, 0).trim_end(), "  ~ Compacting");
    assert_eq!(cell_fg(&backend, 4, 0), Some(terminal(theme.muted)));
    Ok(())
}
