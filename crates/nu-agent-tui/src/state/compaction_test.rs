//! Compaction-domain reducer tests: compaction block rendering, streaming
//! summary projection, and completion/failure bookkeeping. Assertions moved
//! 1:1 from the former `interaction/reducer_test.rs` `reduce_ui_event_impl`
//! effect tests, driven through `CompactionState::reduce_compaction_event`.

use crate::interaction::reducer::{ReducerInput, UserAction, reduce_with_cancel_controller};
use crate::state::{AppState, InputState};
use nu_agent_core::bus::CompactionEvent;
use nu_agent_core::transcript::ir::StyleHint;
use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntryKind};

fn busy_state_with_clean_transcript() -> AppState {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("run".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Submit), None);
    let _ = state.activate_next_prompt();
    state.transcript.entries.clear();
    // Simulate handle_llm_start which sets the lock
    state.input_locked = true;
    state
}

fn reduce_compaction(state: &mut AppState, event: CompactionEvent) -> bool {
    state.compaction.reduce_compaction_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.scroll,
        event,
    )
}

fn started(source: &str) -> CompactionEvent {
    CompactionEvent::Started {
        source: source.to_string(),
    }
}

fn completed(source: &str, summary_preview: &str, summary_body: &str) -> CompactionEvent {
    CompactionEvent::Completed {
        source: source.to_string(),
        summary_preview: summary_preview.to_string(),
        summary_body: summary_body.to_string(),
    }
}

fn chunk(source: &str, delta: &str, aggregated: &str) -> CompactionEvent {
    CompactionEvent::SummaryChunk {
        source: source.to_string(),
        delta: delta.to_string(),
        aggregated: aggregated.to_string(),
    }
}

#[test]
fn compaction_summary_is_rendered_in_transcript() {
    let mut state = AppState::default();

    reduce_compaction(
        &mut state,
        completed(
            "slash_compact",
            "short summary preview",
            "full summary body",
        ),
    );

    let lines = state
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Compaction".to_string()));
    assert!(lines.contains(&"full summary body".to_string()));
}

#[test]
fn compaction_artifact_renders_as_single_markdown_block() {
    let mut state = AppState::default();

    reduce_compaction(
        &mut state,
        completed("slash_compact", "preview", "## Summary\n- one\n- two"),
    );

    // Raw text for non-markdown entries (Compaction header is a SystemMessage)
    let raw_texts: Vec<String> = state
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect();
    assert!(raw_texts.contains(&"Compaction".to_string()));

    // Project markdown entries to verify heading renders as "Summary"
    let projected_texts: Vec<String> = state
        .transcript
        .entries
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert!(
        projected_texts.iter().any(|l| l.contains("Summary")),
        "projected output should contain 'Summary': {projected_texts:?}"
    );
    assert!(
        !raw_texts
            .iter()
            .any(|line| line.starts_with("[compaction source="))
    );
}

#[test]
fn compaction_artifact_does_not_double_wrap_summary_heading() {
    let mut state = AppState::default();

    reduce_compaction(
        &mut state,
        completed("slash_compact", "## Summary", "## Summary\n- single"),
    );

    // Project all entries and count how many contain "Summary" text
    let summary_count = state
        .transcript
        .entries
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .filter(|projected| projected.trim() == "Summary")
        .count();
    assert_eq!(summary_count, 1);
}

#[test]
fn compaction_artifact_preserves_bullets_without_duplication() {
    let mut state = AppState::default();

    reduce_compaction(
        &mut state,
        completed("auto_threshold", "preview", "- alpha\n- beta"),
    );

    // Project all entries and check bullet items appear exactly once
    let projected_texts: Vec<String> = state
        .transcript
        .entries
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• alpha"))
            .count(),
        1,
        "alpha bullet should appear exactly once: {projected_texts:?}"
    );
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• beta"))
            .count(),
        1,
        "beta bullet should appear exactly once: {projected_texts:?}"
    );
    assert!(!projected_texts.iter().any(|line| line.contains("• •")));
}

#[test]
fn compaction_block_completion_hides_source_and_explanatory_copy() {
    let mut state = AppState::default();

    reduce_compaction(
        &mut state,
        completed("auto_threshold", "preview", "## Summary\ncontent line"),
    );

    let raw_texts: Vec<String> = state
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect();

    let projected_texts: Vec<String> = state
        .transcript
        .entries
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();

    assert!(raw_texts.contains(&"Compaction".to_string()));
    assert!(
        projected_texts.iter().any(|l| l.contains("Summary")),
        "projected output should contain Summary: {projected_texts:?}"
    );
    assert!(
        projected_texts.iter().any(|l| l.contains("content line")),
        "projected output should contain content line: {projected_texts:?}"
    );
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
}

#[test]
fn compaction_block_header_is_concise_without_artifact_label() {
    let mut state = AppState::default();

    reduce_compaction(&mut state, started("auto_threshold"));

    let lines = state
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Compaction".to_string()));
    assert!(!lines.contains(&"Compaction artifact".to_string()));
}

#[test]
fn compaction_block_summary_rendering_remains_clean_after_copy_removal() {
    let mut state = AppState::default();

    reduce_compaction(&mut state, started("slash_compact"));
    reduce_compaction(
        &mut state,
        completed("slash_compact", "preview", "## Summary\n- alpha\n- beta"),
    );

    // Project all entries and check projected output
    let projected_texts: Vec<String> = state
        .transcript
        .entries
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.trim() == "Summary")
            .count(),
        1,
        "Summary should appear exactly once: {projected_texts:?}"
    );
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• alpha"))
            .count(),
        1,
        "alpha bullet should appear once: {projected_texts:?}"
    );
    assert_eq!(
        projected_texts
            .iter()
            .filter(|l| l.contains("• beta"))
            .count(),
        1,
        "beta bullet should appear once: {projected_texts:?}"
    );
}

#[test]
fn compaction_metadata_not_included_in_future_prompt_history() {
    let mut state = AppState::default();

    reduce_compaction(&mut state, started("slash_compact"));
    reduce_compaction(
        &mut state,
        completed("slash_compact", "preview", "persisted summary body"),
    );

    assert_eq!(
        state.transcript.entries[0].text(),
        "Compaction",
        "metadata is transcript UI chrome, not session system summary payload"
    );
    assert!(
        state
            .transcript
            .entries
            .iter()
            .any(|line| line.text() == "persisted summary body")
    );
}

#[test]
fn compaction_noop_does_not_claim_persisted_summary() {
    let mut state = AppState::default();

    reduce_compaction(&mut state, started("auto_threshold"));
    reduce_compaction(
        &mut state,
        completed("auto_threshold", "preview", "(empty summary)"),
    );

    let lines = state
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();

    assert!(lines.contains(&"(empty summary)".to_string()));
    assert!(!lines.iter().any(|line| line.contains("source=")));
    assert!(!lines.iter().any(|line| line.contains(
        "metadata above is UI diagnostic only and NOT included in future LLM prompt history"
    )));
    assert!(!lines.iter().any(|line| line.contains(
        "Summary text below is persisted as system summary and IS included in future history"
    )));
}

#[test]
fn compaction_block_renders_for_slash_and_auto_triggers() {
    let mut state = AppState::default();

    for source in ["slash_compact", "auto_threshold"] {
        reduce_compaction(&mut state, started(source));
        reduce_compaction(
            &mut state,
            completed(source, "preview", &format!("summary from {source}")),
        );
    }

    let lines = state
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"summary from slash_compact".to_string()));
    assert!(lines.contains(&"summary from auto_threshold".to_string()));
}

#[test]
fn compaction_completed_clears_status_line() {
    let mut state = busy_state_with_clean_transcript();
    state.status.message.set_message("Thinking...");

    reduce_compaction(&mut state, completed("test", "...", "summary"));

    assert!(
        state.status.message.status_line().is_empty(),
        "status_line should be cleared after CompactionCompleted, got: {:?}",
        state.status.message.status_line()
    );
}

#[test]
fn compaction_completed_resets_latest_total_tokens() {
    let mut state = busy_state_with_clean_transcript();
    // Simulate pre-compaction state: token usage is known
    state.status.tokens.latest_total_tokens = Some(50_000);

    reduce_compaction(&mut state, completed("test", "...", "summary"));

    assert_eq!(
        state.status.tokens.latest_total_tokens, None,
        "latest_total_tokens should be reset to None after CompactionCompleted"
    );
}

#[test]
fn compaction_failed_clears_status_line() {
    let mut state = busy_state_with_clean_transcript();
    state.status.message.set_message("Thinking...");

    reduce_compaction(
        &mut state,
        CompactionEvent::Failed {
            source: "test".to_string(),
            message: "err".to_string(),
        },
    );

    assert!(
        state.status.message.status_line().is_empty(),
        "status_line should be cleared after CompactionFailed, got: {:?}",
        state.status.message.status_line()
    );
}

#[test]
fn compaction_streaming_renders_progressively() {
    let mut state = AppState::default();

    // Start compaction block
    reduce_compaction(&mut state, started("auto"));

    // Stream 3 chunks with growing aggregated text
    reduce_compaction(&mut state, chunk("auto", "Hello", "Hello"));
    let after_chunk1 = state.transcript.entries.len();
    assert!(after_chunk1 > 1, "should have header + content");

    reduce_compaction(&mut state, chunk("auto", " world", "Hello world"));

    reduce_compaction(&mut state, chunk("auto", " done", "Hello world done"));

    // Finalize
    reduce_compaction(
        &mut state,
        completed("auto", "Hello world done", "Hello world done"),
    );

    // Verify content is present and block is finished
    let lines: Vec<String> = state
        .transcript
        .entries
        .iter()
        .map(|item| item.text())
        .collect();
    assert!(lines.iter().any(|l| l.contains("Hello world done")));
}

#[test]
fn compaction_streaming_truncates_and_reprojects() {
    let mut state = AppState::default();

    reduce_compaction(&mut state, started("auto"));

    // First chunk
    reduce_compaction(&mut state, chunk("auto", "First", "First"));

    // Second chunk — should truncate back and reproject
    reduce_compaction(&mut state, chunk("auto", " Second", "First Second"));

    // Should NOT have both "First" standalone AND "First Second"
    // The re-projection replaces, not appends
    let lines: Vec<String> = state
        .transcript
        .entries
        .iter()
        .map(|item| item.text())
        .collect();
    let first_only_count = lines
        .iter()
        .filter(|l| l.contains("First") && !l.contains("Second"))
        .count();
    assert_eq!(first_only_count, 0, "old partial render should be replaced");
}

#[test]
fn compaction_streaming_empty_chunks_ignored() {
    let mut state = AppState::default();

    reduce_compaction(&mut state, started("auto"));
    let after_start = state.transcript.entries.len();

    // Empty chunk
    reduce_compaction(&mut state, chunk("auto", "", ""));

    // Should not have added any content lines
    assert_eq!(state.transcript.entries.len(), after_start);
    assert!(state.transcript.summary_stream_start.is_none());
}

#[test]
fn compaction_completed_clears_streaming_state() {
    let mut state = AppState::default();

    reduce_compaction(&mut state, started("auto"));
    reduce_compaction(&mut state, chunk("auto", "text", "text"));
    assert!(state.transcript.summary_stream_start.is_some());

    reduce_compaction(&mut state, completed("auto", "text", "text"));

    assert!(
        state.transcript.summary_stream_start.is_none(),
        "streaming state must be cleared after CompactionCompleted"
    );
}

// region:    --- Raw markdown projection (moved from task_4a_tests)

/// Return raw markdown strings stored in all Assistant ProseMessage entries.
fn assistant_markdown_entries(state: &AppState) -> Vec<String> {
    state
        .transcript
        .entries
        .iter()
        .filter_map(|e| {
            if let TranscriptEntryKind::Assistant(ProseMessage { markdown }) = &e.kind {
                Some(markdown.clone())
            } else {
                None
            }
        })
        .collect()
}

/// Project a markdown string and return all (text, hint) pairs from it.
fn project_spans(markdown: &str) -> Vec<(String, StyleHint)> {
    crate::markdown::render_markdown_lines(markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .map(|s| (s.text, s.hint))
        .collect()
}

#[test]
fn compaction_chunk_with_italic_emits_md_italic() {
    let mut state = AppState::default();
    reduce_compaction(
        &mut state,
        chunk("history", "summary *italic*", "summary *italic*"),
    );
    assert!(assistant_markdown_entries(&state).iter().any(|md| {
        project_spans(md)
            .iter()
            .any(|(t, h)| t == "italic" && matches!(h, StyleHint::MdItalic))
    }));
}

#[test]
fn compaction_completed_fenced_body_renders_markdown_not_raw() {
    let mut state = AppState::default();
    reduce_compaction(&mut state, started("auto"));
    reduce_compaction(
        &mut state,
        completed(
            "auto",
            "",
            "```\n## Work State\n### Completed\n- Mapped c5t notes for project `63e90e73`; confirmed `7722bef9`.\n```",
        ),
    );

    let markdowns = assistant_markdown_entries(&state);
    let concat: String = markdowns.join("\n");
    let projected = project_spans(&concat);
    // Fenced wrapper must not cause raw markdown to leak into the visible body.
    assert!(
        projected.iter().any(|(t, _)| t == "Work State"),
        "heading text must render; got: {projected:?}"
    );
    assert!(
        projected.iter().any(|(t, _)| t.starts_with('•')),
        "bullet marker must render as '•'; got: {projected:?}"
    );
    assert!(
        !projected.iter().any(|(t, _)| t.starts_with("##")),
        "raw '##' must not appear in rendered body; got: {projected:?}"
    );
}

// endregion: --- Raw markdown projection (moved from task_4a_tests)
