use crate::agent::ui::tui::state::{ToolCallStatus, TranscriptLine, TranscriptLineStatus, TranscriptRole};

use super::render_transcript_lines_for_test;

#[test]
fn tool_completion_renders_tick_without_done_text_token() {
    let line = TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[k8s__list_pods] args={\"namespace\":\"prod\"} · done".to_string(),
        rendered: None,
    };

    let rendered =
        render_transcript_lines_for_test(line, Some(TranscriptLineStatus::Tool(ToolCallStatus::Done)), 0);

    assert!(rendered[0].spans.iter().any(|span| span.content.contains("✓")));
    assert!(!rendered[0]
        .spans
        .iter()
        .any(|span| span.content.contains("· done")));
    assert!(rendered[0]
        .spans
        .iter()
        .any(|span| span.content.contains("args={\"namespace\":\"prod\"}")));
}

#[test]
fn tool_failure_keeps_failed_text_token_and_failure_indicator() {
    let line = TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[k8s__list_pods] args={\"namespace\":\"prod\"} · failed".to_string(),
        rendered: None,
    };

    let rendered = render_transcript_lines_for_test(
        line,
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Failed)),
        0,
    );

    assert!(rendered[0].spans.iter().any(|span| span.content.contains("✕")));
    assert!(rendered[0]
        .spans
        .iter()
        .any(|span| span.content.contains("· failed")));
}
