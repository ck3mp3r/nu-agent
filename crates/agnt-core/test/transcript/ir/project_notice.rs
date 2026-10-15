use super::*;

// ── BlockSource::project — notice / banner / spacer / tool display ──────────

/// Notice, banner, spacer, and tool-display sources each own their projection.
#[test]
fn project_notice_banner_spacer_tool_display_table() -> Result<()> {
    // -- Setup & Fixtures
    let notice = BlockSource::Notice {
        kind: NoticeKind::Compaction,
        text: "compacted 5 blocks".to_string(),
    };
    let banner = BlockSource::Banner {
        text: "line1\nline2".to_string(),
    };
    let lines = vec![
        ContentLine::single("diff a.rs".to_string(), StyleHint::DiffAdd),
        ContentLine::single("context".to_string(), StyleHint::Normal),
    ];
    let tool_display = BlockSource::ToolDisplay {
        lines: lines.clone(),
    };

    // -- Exec
    let notice_projected = notice.project(80);
    let banner_projected = banner.project(80);
    let spacer_projected = BlockSource::Spacer.project(80);
    let tool_display_projected = tool_display.project(80);

    // -- Check: a notice is a single Normal line (the lane role style).
    assert_eq!(notice_projected.len(), 1);
    assert_eq!(notice_projected[0].spans.len(), 1);
    assert_eq!(notice_projected[0].spans[0].text, "compacted 5 blocks");
    assert_eq!(notice_projected[0].spans[0].hint, StyleHint::Normal);

    // -- Check: a banner is one Normal line per source line.
    assert_eq!(banner_projected.len(), 2);
    assert_eq!(banner_projected[0].spans[0].text, "line1");
    assert_eq!(banner_projected[1].spans[0].text, "line2");
    assert!(
        banner_projected
            .iter()
            .all(|l| l.spans[0].hint == StyleHint::Normal),
        "banner lines are Normal"
    );

    // -- Check: a spacer is exactly one empty line.
    assert_eq!(spacer_projected, vec![ContentLine::empty()]);

    // -- Check: a tool display clones its stored lines verbatim.
    assert_eq!(
        tool_display_projected, lines,
        "ToolDisplay must clone its lines verbatim"
    );
    Ok(())
}
