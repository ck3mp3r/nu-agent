use super::support::*;
use agnt_render::transcript::ir::NoticeKind;
use agnt_render::transcript::renderer::ItemStatus;

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

/// WHEN the task widget renders a compaction task, THE row SHALL carry the
/// compaction lane marker, the status indicator, and the muted call line.
#[test]
fn test_widgets_notice_compaction_task_lane_and_color() -> Result<()> {
    // -- Setup & Fixtures
    let entry = task_entry("auto", ItemStatus::InProgress);
    let theme = test_theme();

    // -- Exec
    let (backend, rows) = render_at(&entry, 0)?;

    // -- Check: the marker is the fold glyph, the indicator sits between it and
    // the name, and the line paints muted.
    assert_eq!(rows, 1);
    assert_eq!(
        row_text(&backend, 0).trim_end(),
        "  \u{2261} \u{280b} Compaction · auto"
    );
    assert_eq!(cell_fg(&backend, 2, 0), Some(terminal(theme.muted)));
    assert_eq!(cell_fg(&backend, 6, 0), Some(terminal(theme.muted)));
    Ok(())
}
