use super::*;

// ── BlockSource::project — notice / banner / spacer / tool display ──────────

/// A notice projects to a single line styled with the Normal hint (the lane
/// role style), matching the pre-refactor `SystemMessage` styling.
#[test]
fn project_notice_is_single_normal_line() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Notice {
        kind: NoticeKind::Compaction,
        text: "compacted 5 blocks".to_string(),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].spans.len(), 1);
    assert_eq!(projected[0].spans[0].text, "compacted 5 blocks");
    assert_eq!(projected[0].spans[0].hint, StyleHint::Normal);
    Ok(())
}

/// A banner projects one Normal line per source line.
#[test]
fn project_banner_is_one_normal_line_per_source_line() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Banner {
        text: "line1\nline2".to_string(),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].spans[0].text, "line1");
    assert_eq!(projected[1].spans[0].text, "line2");
    assert!(
        projected
            .iter()
            .all(|l| l.spans[0].hint == StyleHint::Normal),
        "banner lines are Normal"
    );
    Ok(())
}

/// A spacer projects to exactly one empty content line.
#[test]
fn project_spacer_is_one_empty_line() {
    // -- Exec
    let projected = BlockSource::Spacer.project(80);

    // -- Check
    assert_eq!(projected, vec![ContentLine::empty()]);
}

/// A tool display projects to a clone of its stored lines.
#[test]
fn project_tool_display_clones_lines() {
    // -- Setup & Fixtures
    let lines = vec![
        ContentLine::single("diff a.rs".to_string(), StyleHint::DiffAdd),
        ContentLine::single("context".to_string(), StyleHint::Normal),
    ];
    let source = BlockSource::ToolDisplay {
        lines: lines.clone(),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    assert_eq!(
        projected, lines,
        "ToolDisplay must clone its lines verbatim"
    );
}
