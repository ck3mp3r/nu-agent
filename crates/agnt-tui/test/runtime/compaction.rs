use super::*;

#[test]
fn hydration_compaction_creates_block_structure() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new(
            "compaction",
            "## Summary\n- point one\n- point two",
        )],
        None,
    );

    // The compaction block header ("Compaction") should be present in transcript
    let has_compaction_header = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .any(|b| b.source.plain_text() == "Compaction");
    assert!(
        has_compaction_header,
        "expected compaction block header in transcript"
    );
}

#[test]
fn hydration_compaction_renders_markdown_body() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new(
            "compaction",
            "## Summary\n- alpha\n- beta",
        )],
        None,
    );

    // After the raw-markdown refactor, the compaction body is stored as raw markdown.
    // Projection (sanitization + rendering) happens at render time.
    // Test by projecting the stored markdown and checking the output.
    let projected_texts: Vec<String> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.source.plain_text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();

    // Raw markdown markers should NOT appear in the projected output
    assert!(
        !projected_texts.iter().any(|t| t.contains("## ")),
        "raw markdown heading marker should not appear in projected output: {projected_texts:?}"
    );
    assert!(
        !projected_texts.iter().any(|t| t.starts_with("- ")),
        "raw markdown list marker should not appear in projected output: {projected_texts:?}"
    );
    // Rendered content should be present
    assert!(
        projected_texts.iter().any(|t| t.contains("Summary")),
        "rendered heading text should appear in projected output: {projected_texts:?}"
    );
    assert!(
        projected_texts.iter().any(|t| t.contains("alpha")),
        "rendered list item text should appear in projected output: {projected_texts:?}"
    );
}

#[test]
fn hydration_compaction_fenced_body_renders_markdown_not_raw() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new(
            "compaction",
            "```\n## Work State\n### Completed\n- Mapped `63e90e73`; confirmed `7722bef9`.\n```",
        )],
        None,
    );

    let projected_texts: Vec<String> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.source.plain_text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();

    assert!(
        !projected_texts.iter().any(|t| t.contains("##")),
        "raw '##' must not appear after hydration of fenced body: {projected_texts:?}"
    );
    assert!(
        projected_texts.iter().any(|t| t.contains("Work State")),
        "heading text must render: {projected_texts:?}"
    );
    assert!(
        projected_texts.iter().any(|t| t.starts_with('•')),
        "bullet marker must render as '•': {projected_texts:?}"
    );
}

#[test]
fn hydration_compaction_empty_summary_shows_block_only() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .hydrate_transcript_from_messages(vec![UiMessageSnapshot::new("compaction", "")], None);

    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect();

    // The compaction header should exist
    assert!(
        texts.contains(&"Compaction".to_string()),
        "expected compaction block header: {texts:?}"
    );

    // Only the header should be present — no body content: the Notice header
    // block is the only Compaction-role block and it carries just the header
    // text.
    let compaction_blocks: Vec<_> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .filter(|b| block_to_role(b) == TranscriptRole::Compaction)
        .collect();
    assert_eq!(
        compaction_blocks.len(),
        1,
        "empty summary should produce the header block only: {compaction_blocks:?}"
    );
    assert_eq!(
        compaction_blocks[0].source.plain_text(),
        "Compaction",
        "the compaction header block must carry only the header text"
    );
}

#[test]
fn hydration_compaction_matches_live_rendering() {
    let summary_body = "## Summary\n- alpha\n- beta";

    // Live path: CompactionStarted + CompactionCompleted via reducer
    let mut live = RuntimeCoordinator::new(120, 30, Some(true));
    live.enqueue_ui_event(UiEvent::CompactionStarted {
        source: "history".to_string(),
    });
    live.drain_transport();
    live.enqueue_ui_event(UiEvent::CompactionCompleted {
        source: "history".to_string(),
        summary_preview: "preview".to_string(),
        summary_body: summary_body.to_string(),
    });
    live.drain_transport();

    // Hydration path: UiMessageSnapshot with role "compaction"
    let mut hydrated = RuntimeCoordinator::new(120, 30, Some(true));
    hydrated.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("compaction", summary_body)],
        None,
    );

    let live_texts: Vec<String> = live
        .state()
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect();
    let hydrated_texts: Vec<String> = hydrated
        .state()
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect();

    assert_eq!(
        live_texts, hydrated_texts,
        "live and hydrated transcript texts should match"
    );

    let live_roles: Vec<TranscriptRole> = live
        .state()
        .transcript
        .blocks()
        .iter()
        .map(block_to_role)
        .collect();
    let hydrated_roles: Vec<TranscriptRole> = hydrated
        .state()
        .transcript
        .blocks()
        .iter()
        .map(block_to_role)
        .collect();

    assert_eq!(
        live_roles, hydrated_roles,
        "live and hydrated transcript roles should match"
    );
}
