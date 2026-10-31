use super::*;

/// A completed compaction renders a concise "Compaction" header plus the
/// summary body as a single markdown block, with no source metadata or
/// explanatory copy.
#[test]
fn compaction_artifact_renders_summary_without_metadata_or_copy() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_compaction(
        &mut state,
        completed(
            "slash_compact",
            "short summary preview",
            "full summary body",
        ),
    );

    // -- Check
    let raw_texts: Vec<String> = state.transcript.entries().iter().map(entry_text).collect();
    assert!(raw_texts.contains(&"Compaction".to_string()));
    assert!(raw_texts.contains(&"full summary body".to_string()));
    assert!(!raw_texts.iter().any(|line| line.contains("source=")));
    assert!(
        !raw_texts
            .iter()
            .any(|line| line.contains("metadata above is UI diagnostic only"))
    );
    assert!(
        !raw_texts
            .iter()
            .any(|line| line.contains("persisted as system summary"))
    );

    // -- Setup & Fixtures: markdown summary.
    let mut markdown = AppState::default();
    reduce_compaction(
        &mut markdown,
        completed("slash_compact", "preview", "## Summary\n- one\n- two"),
    );

    // -- Check: the heading renders as "Summary" and no raw source leaks.
    let projected_texts: Vec<String> = markdown
        .transcript
        .entries()
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&entry_text(line), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert!(projected_texts.iter().any(|l| l.contains("Summary")));
    assert!(
        !markdown
            .transcript
            .entries()
            .iter()
            .any(|line| entry_text(line).starts_with("[compaction source="))
    );

    // -- Setup & Fixtures: explanatory copy must be absent.
    let mut copy = AppState::default();
    reduce_compaction(
        &mut copy,
        completed("auto_threshold", "preview", "## Summary\ncontent line"),
    );

    // -- Check
    let raw_texts: Vec<String> = copy.transcript.entries().iter().map(entry_text).collect();
    let projected_texts: Vec<String> = copy
        .transcript
        .entries()
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&entry_text(line), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert!(raw_texts.contains(&"Compaction".to_string()));
    assert!(projected_texts.iter().any(|l| l.contains("Summary")));
    assert!(projected_texts.iter().any(|l| l.contains("content line")));
    assert!(!raw_texts.iter().any(|line| line.contains("source=")));
    Ok(())
}

/// Compaction summary content is never duplicated: headings wrap once and
/// bullets appear exactly once.
#[test]
fn compaction_artifact_does_not_duplicate_summary_content() -> Result<()> {
    // -- Setup & Fixtures
    let mut heading = AppState::default();
    reduce_compaction(
        &mut heading,
        completed("slash_compact", "## Summary", "## Summary\n- single"),
    );

    // -- Check: the heading appears exactly once.
    let summary_count = heading
        .transcript
        .entries()
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&entry_text(line), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .filter(|projected| projected.trim() == "Summary")
        .count();
    assert_eq!(summary_count, 1);

    // -- Setup & Fixtures: bullets.
    let mut bullets = AppState::default();
    reduce_compaction(
        &mut bullets,
        completed("auto_threshold", "preview", "- alpha\n- beta"),
    );

    // -- Check: each bullet appears exactly once, no doubled markers.
    let projected_texts: Vec<String> = bullets
        .transcript
        .entries()
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&entry_text(line), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• alpha"))
            .count(),
        1
    );
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• beta"))
            .count(),
        1
    );
    assert!(!projected_texts.iter().any(|line| line.contains("• •")));

    // -- Setup & Fixtures: start then complete.
    let mut lifecycle = AppState::default();
    reduce_compaction(&mut lifecycle, started("slash_compact"));
    reduce_compaction(
        &mut lifecycle,
        completed("slash_compact", "preview", "## Summary\n- alpha\n- beta"),
    );

    // -- Check
    let projected_texts: Vec<String> = lifecycle
        .transcript
        .entries()
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&entry_text(line), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.trim() == "Summary")
            .count(),
        1
    );
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• alpha"))
            .count(),
        1
    );
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• beta"))
            .count(),
        1
    );
    Ok(())
}

/// The compaction header is the concise "Compaction" label, never the
/// "Compaction artifact" label.
#[test]
fn compaction_block_header_is_concise_without_artifact_label() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_compaction(&mut state, started("auto_threshold"));

    // -- Check
    let lines = state
        .transcript
        .entries()
        .iter()
        .map(entry_text)
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Compaction".to_string()));
    assert!(!lines.contains(&"Compaction artifact".to_string()));
    Ok(())
}
