use super::*;

use crate::runtime::status::content::model_activity_label;
use crate::test_support::session_switched;

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

    // The compaction task line ("Compaction · history") should be present.
    let has_compaction_header = coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .any(|b| entry_text(b) == "Compaction · history");
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
        .entries()
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&entry_text(line), None))
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
        .entries()
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&entry_text(line), None))
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
        .entries()
        .iter()
        .map(entry_text)
        .collect();

    // The compaction call line should exist
    assert!(
        texts.contains(&"Compaction · history".to_string()),
        "expected compaction call line: {texts:?}"
    );

    // Only the call line should be present — no body content: the Task header
    // block is the only Compaction-role block and it carries just the line
    // text.
    let blocks = coordinator.state().transcript.entries();
    let compaction_blocks: Vec<_> = blocks
        .iter()
        .filter(|b| entry_to_role(b) == TranscriptRole::Compaction)
        .collect();
    assert_eq!(
        compaction_blocks.len(),
        1,
        "empty summary should produce the call line only: {compaction_blocks:?}"
    );
    assert_eq!(
        entry_text(compaction_blocks[0]),
        "Compaction · history",
        "the compaction call line must carry only the line text"
    );
}

#[test]
fn uncompacted_record_closes_the_block_and_reports_the_undo() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    coordinator.fold_record(&SessionRecord::Compaction(CompactionRecord::Started {
        source: CompactionSource::Manual,
    }));
    coordinator.fold_record(&SessionRecord::Compaction(CompactionRecord::Uncompacted {
        source: CompactionSource::Manual,
    }));

    // -- Check
    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .map(entry_text)
        .collect();
    assert!(
        texts.contains(
            &"Compaction undone: the restored messages are the context the next turn runs on"
                .to_string()
        ),
        "the undo must leave the drawer's own notice row, got: {texts:?}"
    );
    assert!(
        !coordinator.state().compaction.in_progress(),
        "the undone block must not stay in progress: {texts:?}"
    );
}

/// WHEN the surface switches away from a session whose sweep was still running,
/// THE status bar SHALL drop the busy indicator it read from that sweep: the
/// block list describes rows the switch's transcript clear dropped, so the
/// arriving session has no work of its own to show.
#[test]
fn switch_settles_the_departed_sessions_compaction_indicator() {
    // -- Setup & Fixtures: a session compacting at rest.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.fold_record(&compaction_started(CompactionSource::Auto));
    assert!(
        coordinator.state().compaction.in_progress(),
        "the fixture is compacting"
    );
    assert_eq!(
        model_activity_label(coordinator.state()),
        "busy",
        "the idle phase still reads busy while a sweep runs"
    );

    // -- Exec
    coordinator.fold_record(&session_switched());

    // -- Check: the arriving surface is at rest.
    assert!(
        !coordinator.state().compaction.in_progress(),
        "the departed session's sweep does not cross the boundary"
    );
    assert_eq!(
        model_activity_label(coordinator.state()),
        "idle",
        "the busy indicator belongs to the session that left"
    );
}

#[test]
fn completed_record_without_source_draws_the_replay_label() {
    // -- Setup & Fixtures
    let summary_body = "## Summary\n- alpha";
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    coordinator.fold_record(&SessionRecord::Compaction(CompactionRecord::Completed {
        source: None,
        summary: summary_body.to_string(),
    }));

    // -- Check: the record names no source, so the drawer labels the row the way
    // it labels a replayed one, and the summary body stands.
    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .map(entry_text)
        .collect();
    assert!(
        texts.contains(&"Compaction · history".to_string()),
        "a record that names no source draws as history: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("- alpha")),
        "the summary body renders: {texts:?}"
    );
}

#[test]
fn hydration_compaction_matches_live_rendering() {
    let summary_body = "## Summary\n- alpha\n- beta";

    // Live path: the sweep's start and completion records.
    let mut live = RuntimeCoordinator::new(120, 30, Some(true));
    live.fold_record(&SessionRecord::Compaction(CompactionRecord::Started {
        source: CompactionSource::Auto,
    }));
    live.fold_record(&SessionRecord::Compaction(CompactionRecord::Completed {
        source: Some(CompactionSource::Auto),
        summary: summary_body.to_string(),
    }));

    // Hydration path: UiMessageSnapshot with role "compaction"
    let mut hydrated = RuntimeCoordinator::new(120, 30, Some(true));
    hydrated.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("compaction", summary_body)],
        None,
    );

    let live_texts: Vec<String> = live
        .state()
        .transcript
        .entries()
        .iter()
        .map(entry_text)
        .collect();
    let hydrated_texts: Vec<String> = hydrated
        .state()
        .transcript
        .entries()
        .iter()
        .map(entry_text)
        .collect();

    // The drawer labels the two rows by their source: a live sweep reads its
    // own source, a rehydrated one reads `history`. The shape and the body
    // match.
    assert_eq!(live_texts.len(), hydrated_texts.len());
    assert!(
        live_texts.contains(&"Compaction · auto".to_string()),
        "live row reads its source: {live_texts:?}"
    );
    assert!(
        hydrated_texts.contains(&"Compaction · history".to_string()),
        "rehydrated row reads history: {hydrated_texts:?}"
    );
    for texts in [&live_texts, &hydrated_texts] {
        assert!(
            texts.iter().any(|text| text.contains("- alpha")),
            "the summary body renders in both: {texts:?}"
        );
    }

    let live_roles: Vec<TranscriptRole> = live
        .state()
        .transcript
        .entries()
        .iter()
        .map(entry_to_role)
        .collect();
    let hydrated_roles: Vec<TranscriptRole> = hydrated
        .state()
        .transcript
        .entries()
        .iter()
        .map(entry_to_role)
        .collect();

    assert_eq!(
        live_roles, hydrated_roles,
        "live and hydrated transcript roles should match"
    );
}
