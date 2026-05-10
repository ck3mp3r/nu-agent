use crate::agent::ui::tui::state::{ToolCallStatus, TranscriptLine, TranscriptLineStatus, TranscriptRole};

use super::{
    lane_prefix_spans_for_test, parse_persisted_tool_status_line_for_test,
    render_transcript_lines_for_test,
};

#[test]
fn tool_row_label_renders_without_tool_brackets_prefix() {
    let line = TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[nu__run] args={\"command\":\"version\"}".to_string(),
        rendered: None,
    };

    let rendered = render_transcript_lines_for_test(line, None, 0);
    let row_text = rendered[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(row_text.contains("nu__run"));
    assert!(!row_text.contains("tool[nu__run]"));
    assert!(row_text.contains("args={\"command\":\"version\"}"));
}

#[test]
fn tool_lane_prefix_uses_cog_wheel_icon() {
    let spans = lane_prefix_spans_for_test(TranscriptRole::Tool, false);
    let text = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(text, "  ⚙ ");
}

#[test]
fn tool_row_done_and_failed_indicators_remain_unchanged_after_label_cleanup() {
    let done_line = TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[nu__run] args={\"command\":\"version\"} · done".to_string(),
        rendered: None,
    };
    let done_rendered =
        render_transcript_lines_for_test(done_line, Some(TranscriptLineStatus::Tool(ToolCallStatus::Done)), 0);
    let done_text = done_rendered[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(done_text.contains("✓"));
    assert!(!done_text.contains("· done"));
    assert!(done_text.contains("nu__run"));
    assert!(done_text.contains("args={\"command\":\"version\"}"));

    let failed_line = TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[nu__run] args={\"command\":\"version\"} · failed".to_string(),
        rendered: None,
    };
    let failed_rendered = render_transcript_lines_for_test(
        failed_line,
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Failed)),
        0,
    );
    let failed_text = failed_rendered[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(failed_text.contains("✕"));
    assert!(failed_text.contains("· failed"));
    assert!(failed_text.contains("nu__run"));
    assert!(!failed_text.contains("tool[nu__run]"));
}

#[test]
fn persisted_tool_line_hydration_still_parses_tool_name_and_metadata() {
    let parsed = parse_persisted_tool_status_line_for_test(
        "tool[nu__run] args={\"command\":\"version\"} · done",
    );

    assert_eq!(
        parsed,
        Some(("nu__run", "{\"command\":\"version\"}", true))
    );
}

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
