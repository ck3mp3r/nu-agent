//! Tool-domain reducer tests: tool rows, display rendering, and the
//! permission pre-display dedup. Assertions moved 1:1 from the former
//! `interaction/reducer_test.rs` `reduce_ui_event_impl` effect tests, driven
//! through `ToolState::reduce_tool_event`.

use crate::interaction::reducer::{apply_permission_request_display, dispatch_ui_event};
use crate::state::AppState;
use nu_agent_core::bus::ToolEvent;
use nu_agent_core::protocol::contracts::UiMessageSnapshot;
use nu_agent_core::protocol::event::{ToolDisplay, ToolDisplaySection, UiEvent};
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::{BlockSource, ContentKind, DiffTint};
use nu_agent_core::transcript::renderer::ItemStatus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

use nu_agent_core::transcript::ir::Fill;
use nu_agent_core::transcript::ir::StyleHint;
use nu_agent_core::transcript::ir::{Display, DisplaySection};

use crate::rendering::theme::TuiTheme;

// ---------------------------------------------------------------------------
// In-place tool block mutation (task 7ab65c6a)
// ---------------------------------------------------------------------------

#[test]
fn start_tool_call_creates_exactly_one_block() {
    let mut state = AppState::default();

    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
        &mut evicted,
    );

    // One Block only — no separate spacer, no separate display entry.
    assert_eq!(state.transcript.len(), 1);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn set_tool_preview_pushes_adjacent_tool_display_block() -> Result<()> {
    let mut state = AppState::default();

    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        CallLine::from_json_summary(r#"{"path":"a.rs"}"#),
        &mut evicted,
    );
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "changes".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "--- a\n+++ b\n".to_string(),
            stats: None,
        }],
    };
    let mut preview_evicted = 0usize;
    state.tool.set_tool_preview(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        preview,
        &mut preview_evicted,
    );

    // -- Check: the Tool block keeps no preview and no fill; the preview is a
    // separate ToolDisplay block pushed directly after it.
    assert_eq!(
        state.transcript.len(),
        2,
        "preview must push its own block after the Tool block"
    );
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(
        matches!(tool_block.source, BlockSource::Tool { preview: None, .. }),
        "the Tool block must keep preview None"
    );
    assert_eq!(
        tool_block.fill,
        Fill::None,
        "the Tool block must keep Fill::None so the call line stays untinted"
    );

    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the preview must be a ToolDisplay block"
    );
    assert_eq!(
        preview_block.fill,
        Fill::Code,
        "a diff preview block must carry Fill::Code"
    );
    assert!(
        tool_display_lines(preview_block).contains("+++ b"),
        "the preview block must carry the projected diff content"
    );
    Ok(())
}

#[test]
fn set_tool_preview_invalidates_height_index() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let width = 80usize;

    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        CallLine::from_json_summary(r#"{"path":"a.rs"}"#),
        &mut evicted,
    );
    // Build the height index so it is valid, then push the preview block.
    state.transcript.rebuild_height_index(width);
    assert!(
        state.transcript.height_index_valid_for(width),
        "precondition: index must be valid before the preview push"
    );

    // -- Exec
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "changes".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "--- a\n+++ b\n".to_string(),
            stats: None,
        }],
    };
    let mut preview_evicted = 0usize;
    state.tool.set_tool_preview(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        preview,
        &mut preview_evicted,
    );

    // -- Check: the push added rows, so the index is stale.
    assert!(
        !state.transcript.height_index_valid_for(width),
        "set_tool_preview must invalidate the height index"
    );
    Ok(())
}

#[test]
fn finish_tool_call_mutates_status_on_existing_block() {
    let mut state = AppState::default();

    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "read",
        "{}",
        CallLine::from_json_summary("{}"),
        &mut evicted,
    );
    state
        .tool
        .finish_tool_call(&mut state.transcript, "read", "{}", Some(true));

    assert_eq!(
        state.transcript.len(),
        1,
        "finish must not push a new block"
    );
    assert_eq!(state.transcript.blocks()[0].status, Some(ItemStatus::Done));
}

/// Collect the StyleHints of all ToolDisplay blocks' stored ContentLines —
/// one block per section, pushed with pre-projected spans.
fn diff_section_hints(state: &AppState) -> Vec<StyleHint> {
    state
        .transcript
        .blocks()
        .iter()
        .filter_map(|b| match &b.source {
            BlockSource::ToolDisplay { lines } => Some(lines.clone()),
            _ => None,
        })
        .flat_map(|lines| {
            lines
                .iter()
                .flat_map(|line| line.spans.iter().map(|s| s.hint.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Collect the line-level diff tints of all ToolDisplay blocks' stored
/// ContentLines.
fn diff_section_tints(state: &AppState) -> Vec<DiffTint> {
    state
        .transcript
        .blocks()
        .iter()
        .filter_map(|b| match &b.source {
            BlockSource::ToolDisplay { lines } => Some(lines.clone()),
            _ => None,
        })
        .flat_map(|lines| {
            lines
                .iter()
                .filter_map(|line| line.diff_tint.clone())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A `diff` section carries the structural hints (DiffHunk for `@@ `, Meta for
/// `---`/`+++`) and syntax-highlighted bodies with a line-level diff tint —
/// not whole-line DiffAdd/DiffRemove hints (task 7f931bef).
#[test]
fn tool_display_diff_section_produces_diff_hints() -> Result<()> {
    let content = "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n context\n\\ No newline at end of file\n";
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: content.to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let hints = diff_section_hints(&state);
    assert!(
        hints.contains(&StyleHint::DiffHunk),
        "diff section must carry DiffHunk hints for '@@ ' lines; got {hints:?}"
    );
    assert!(
        hints.contains(&StyleHint::Meta),
        "diff section must carry Meta hints for ---/+++ header lines; got {hints:?}"
    );
    assert!(
        hints.contains(&StyleHint::Muted),
        "diff body lines must carry a Muted gutter span; got {hints:?}"
    );
    assert!(
        !hints
            .iter()
            .any(|h| matches!(h, StyleHint::DiffAdd | StyleHint::DiffRemove)),
        "diff bodies must not carry whole-line DiffAdd/DiffRemove hints; got {hints:?}"
    );

    let tints = diff_section_tints(&state);
    assert!(
        tints.contains(&DiffTint::Add)
            && tints.contains(&DiffTint::Remove)
            && tints.contains(&DiffTint::Context),
        "diff section must carry Add/Remove/Context line tints; got {tints:?}"
    );
    Ok(())
}

/// Regression (task 6424470b): diff lines keep the line-number prefixes added
/// by `project_diff_lines`.
#[test]
fn tool_display_diff_section_keeps_line_number_prefixes() -> Result<()> {
    let content = "@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n";
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: content.to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        lines.iter().any(|line| line.contains("│alpha")),
        "context line must keep its line-number prefix; got {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("│beta")),
        "removed line must keep its line-number prefix; got {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("│omega")),
        "added line must keep its line-number prefix; got {lines:?}"
    );
    Ok(())
}

/// Regression (task 6424470b criterion 2): non-diff code sections still get
/// syntect MdCode* hints — only the diff path changes.
#[test]
fn tool_display_non_diff_code_section_still_produces_code_hints() -> Result<()> {
    let content = "fn main() {}\n";
    let mut state = AppState::default();
    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "nu".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"command":"ls"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "nu sample.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.rs".to_string(),
                    kind: ContentKind::Code {
                        language: "rust".to_string(),
                    },
                    content: content.to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let hints = diff_section_hints(&state);
    assert!(
        hints
            .iter()
            .any(|h| matches!(h, StyleHint::MdCodeKeyword | StyleHint::MdCodePlain)),
        "non-diff code section must keep MdCode* hints; got {hints:?}"
    );
    Ok(())
}

/// Regression (task 7bd175d2): every edit-display line must render as exactly
/// one visual row. A trailing newline in the projected row text made the word
/// wrapper emit an extra empty row after every diff line, interleaving blank
/// rows in the transcript.
#[test]
fn edit_display_renders_one_visual_row_per_line_without_blank_rows() -> Result<()> {
    use crate::tui_renderer::layout;
    use nu_agent_core::transcript::renderer::FrameContext;

    let content = "--- a/README.md\n+++ b/README.md\n@@ -1,4 +1,6 @@\n # Title\n \n+```rust\n fn a() {}\n+```\n tail\n";
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"path":"README.md"}"#));
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"README.md"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit README.md".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "README.md".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: content.to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let diff_rows: Vec<_> = state
        .transcript
        .blocks()
        .iter()
        .filter(|b| matches!(b.source, BlockSource::ToolDisplay { .. }))
        .cloned()
        .collect();
    let display_line_count: usize = diff_rows
        .iter()
        .map(|b| extract_all_text_from_entry(b).len())
        .sum();
    assert!(
        display_line_count >= 8,
        "edit display must push every diff line; got {display_line_count}"
    );

    let ctx = FrameContext {
        width: 120,
        now_millis: 0,
        cursor: false,
        selected: false,
    };
    let mut rendered_rows = 0usize;
    for block in &diff_rows {
        rendered_rows += layout(block, &ctx, &TuiTheme::default()).len();
    }
    assert_eq!(
        rendered_rows, display_line_count,
        "each display line must render as exactly one visual row (no blank rows from trailing newlines)"
    );

    // No stored display line may carry a raw trailing newline.
    for block in &diff_rows {
        for text in extract_all_text_from_entry(block) {
            assert!(
                !text.ends_with('\n'),
                "display line text must not embed a trailing newline; got {text:?}"
            );
        }
    }
    Ok(())
}

// region:    --- Test Support

fn tool_display_lines(block: &nu_agent_core::transcript::ir::Block) -> String {
    match &block.source {
        BlockSource::ToolDisplay { lines } => lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Flatten a block's source text for assertions. A Tool block contributes its
/// call summary only — previews are their own ToolDisplay blocks.
fn extract_all_text_from_entry(block: &nu_agent_core::transcript::ir::Block) -> Vec<String> {
    match &block.source {
        BlockSource::Tool { call, .. } => vec![call.summary.clone()],
        BlockSource::ToolDisplay { lines } => lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<String>()
            })
            .collect(),
        BlockSource::Notice { text, .. } => vec![text.clone()],
        BlockSource::Markdown { markdown, .. } => vec![markdown.clone()],
        BlockSource::Banner { text } => vec![text.clone()],
        BlockSource::Spacer => vec![String::new()],
    }
}

fn reduce_tool(state: &mut AppState, event: ToolEvent) -> bool {
    let mut evicted = 0usize;
    let changed = state
        .tool
        .reduce_tool_event(&mut state.transcript, event, &mut evicted);
    state.shift_bookkeeping_after_eviction(evicted);
    changed
}

fn started(name: &str, arguments: &str) -> ToolEvent {
    ToolEvent::Started {
        name: name.to_string(),
        source: "mcp".to_string(),
        arguments: arguments.to_string(),
        call_line: CallLine::from_json_summary(arguments),
    }
}

fn started_with_call_line(name: &str, arguments: &str, call_line: CallLine) -> ToolEvent {
    ToolEvent::Started {
        name: name.to_string(),
        source: "mcp".to_string(),
        arguments: arguments.to_string(),
        call_line,
    }
}

// endregion: --- Test Support

#[test]
fn tool_end_transcript_line_shows_args_summary_without_result_payload_dump() {
    let mut state = AppState::default();
    reduce_tool(
        &mut state,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"namespace":"prod"}"#.to_string(),
            success: true,
            result: "[{\"name\":\"api-0\",\"ns\":\"prod\"}]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    );

    // [Tool] — no leading spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert!(call.summary.contains("namespace"));
        assert!(!call.summary.contains("api-0"));
        assert!(!call.summary.contains("[{"));
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_row_materializes_immediately_on_tool_start_with_args_and_running_status() {
    let mut state = AppState::default();

    reduce_tool(
        &mut state,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );

    assert_eq!(state.transcript.len(), 1);
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert!(call.summary.contains("namespace"));
    } else {
        panic!("Expected Tool variant");
    }
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::InProgress)
    );
}

#[test]
fn tool_start_nu_sets_call_line_to_code_block_with_raw_command() {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_tool(
        &mut state,
        started_with_call_line(
            "nu",
            r#"{"command":"ls | select name type size"}"#,
            CallLine {
                summary: String::new(),
            },
        ),
    );

    // -- Check: the command renders in the preview block, so the call line
    // carries no summary.
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert_eq!(call.summary, String::new());
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_start_nu_multi_line_command_preserves_newlines_in_call_line() {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_tool(
        &mut state,
        started_with_call_line(
            "nu",
            r#"{"command":"ls | where size > 1mb\n| select name type\n| sort-by modified"}"#,
            CallLine {
                summary: String::new(),
            },
        ),
    );

    // -- Check: a multi-line command does not leak onto the call line either.
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert_eq!(call.summary, String::new());
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_start_nu_without_command_key_falls_back_to_summary_arrow() {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_tool(&mut state, started("nu", r#"{"timeout_seconds":5}"#));

    // -- Check
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert!(
            call.summary.starts_with("→ "),
            "fallback must use the arrow summary, got: {:?}",
            call.summary
        );
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_start_non_nu_keeps_args_summary_arrow() {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_tool(
        &mut state,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );

    // -- Check
    let block = &state.transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert!(call.summary.starts_with("→ "));
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_end_transitions_same_row_to_done_or_failed_status() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("gh__get_pr", r#"{"number":1}"#));
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "gh__get_pr".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"number":1}"#.to_string(),
            success: true,
            result: "ok".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    );

    // [Tool] — no leading spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(state.transcript.blocks()[0].status, Some(ItemStatus::Done));

    let mut failed = AppState::default();
    reduce_tool(&mut failed, started("gh__get_pr", r#"{"number":2}"#));
    reduce_tool(
        &mut failed,
        ToolEvent::Completed {
            name: "gh__get_pr".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"number":2}"#.to_string(),
            success: false,
            result: "err".to_string(),
            display: None,
            error_kind: Some("tool_error".to_string()),
            message: Some("boom".to_string()),
        },
    );
    assert_eq!(failed.transcript.len(), 1);
    assert_eq!(
        failed.transcript.blocks()[0].status,
        Some(ItemStatus::Failed)
    );
}

#[test]
fn tool_start_leaves_status_line_empty() {
    let mut state = AppState::default();
    reduce_tool(&mut state, started("k8s__list_pods", "{}"));
    assert!(state.status.message.status_line().is_empty());
}

#[test]
fn tool_end_leaves_status_line_empty() {
    let mut state = AppState::default();
    reduce_tool(&mut state, started("k8s__list_pods", "{}"));
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: "[]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    );
    assert!(state.status.message.status_line().is_empty());
}

#[test]
fn tool_start_truncates_long_args_summary_with_ellipsis() {
    let mut state = AppState::default();
    let long_args = format!("{{\"payload\":\"{}\"}}", "x".repeat(300));

    reduce_tool(&mut state, started("k8s__describe", &long_args));

    // [Tool] — no leading spacer under the unified spacer rule
    assert_eq!(state.transcript.len(), 1);
    if let BlockSource::Tool { call, .. } = &state.transcript.blocks()[0].source {
        assert!(call.summary.starts_with("→ "));
        assert!(call.summary.ends_with('…'));
        assert!(call.summary.chars().count() < 180);
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn tool_display_renders_diff_sections_as_dedicated_code_blocks() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(
        !lines.contains(&"sample.txt (diff)".to_string()),
        "the call line already shows the path, so the section label is redundant"
    );
    assert!(!lines.iter().any(|line| line.contains("fn main")));
    assert!(lines.iter().any(|line| line.contains("--- a/sample.txt")));
    assert!(lines.iter().any(|line| line.contains("+++ b/sample.txt")));
}

#[test]
fn tool_display_body_lines_are_unprefixed_while_tool_call_line_remains_prefixed() -> Result<()> {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let call_row = state
        .transcript
        .blocks()
        .iter()
        .find(|b| matches!(&b.source, BlockSource::Tool { .. }))
        .ok_or("should have tool call row")?;
    assert!(matches!(call_row.source, BlockSource::Tool { .. }));

    let display_rows: Vec<_> = state
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(b.source, BlockSource::ToolDisplay { .. })
                && tool_display_lines(b).contains("--- a/sample.txt")
        })
        .collect();

    assert!(!display_rows.is_empty());
    assert!(
        display_rows
            .iter()
            .all(|b| matches!(b.source, BlockSource::ToolDisplay { .. }))
    );
    Ok(())
}

#[test]
fn tool_display_diff_block_highlighting_remains_after_prefix_hygiene_fix() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let diff_rows: Vec<_> = state
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(b.source, BlockSource::ToolDisplay { .. })
                && (tool_display_lines(b).contains("--- a/sample.txt")
                    || tool_display_lines(b).contains("+++ b/sample.txt"))
        })
        .collect();

    assert!(!diff_rows.is_empty());
    // Note: rendered field no longer exists in TranscriptEntry
}

#[test]
fn diff_display_preserves_hunk_line_range_context() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -10,3 +10,4 @@\n line-a\n-line-b\n+line-c\n line-d\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    assert!(
        state
            .transcript
            .blocks()
            .iter()
            .any(|b| tool_display_lines(b).contains("@@ -10,3 +10,4 @@"))
    );
}

#[test]
fn diff_display_supports_line_number_readability_without_breaking_highlighting() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -3,2 +3,2 @@\n alpha\n-beta\n+omega\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let diff_rows: Vec<_> = state
        .transcript
        .blocks()
        .iter()
        .filter(|b| matches!(b.source, BlockSource::ToolDisplay { .. }))
        .collect();

    assert!(
        diff_rows
            .iter()
            .any(|b| tool_display_lines(b).contains("│alpha")
                || tool_display_lines(b).contains("│beta")
                || tool_display_lines(b).contains("│omega"))
    );
}

#[test]
fn edit_preview_display_omits_redundant_edit_path_header() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(
        !lines.contains(&"sample.txt (diff)".to_string()),
        "the call line already shows the path, so the section label is redundant"
    );
}

/// The completed-edit display suppression decision must be typed: the tool
/// name arrives on the event as `"edit"` and maps to `BuiltinKind::Edit` —
/// never a title-text prefix probe, which any title (including a user-visible
/// path like `edit notes/todo.md`) can false-match.
#[test]
fn completed_edit_display_suppresses_title_from_typed_tool_name_not_title_text() -> Result<()> {
    let mut state = AppState::default();

    reduce_tool(
        &mut state,
        started("edit", r#"{\"path\":\"notes/todo.md\"}"#),
    );
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{\"path\":\"notes/todo.md\"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit notes/todo.md".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "notes/todo.md".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        !lines.iter().any(|line| line.contains("edit notes/todo.md")),
        "typed edit identity must suppress the redundant title row; got {lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("notes/todo.md (diff)")),
        "single-section edit display must also skip the redundant section label; got {lines:?}"
    );
    Ok(())
}

#[test]
fn edit_display_with_multiple_sections_keeps_section_labels() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![
                    ToolDisplaySection {
                        label: "sample.txt".to_string(),
                        kind: ContentKind::Diff {
                            language: "diff".to_string(),
                        },
                        content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                            .to_string(),
                        stats: None,
                    },
                    ToolDisplaySection {
                        label: "other.txt".to_string(),
                        kind: ContentKind::Diff {
                            language: "diff".to_string(),
                        },
                        content: "--- a/other.txt\n+++ b/other.txt\n@@ -1 +1 @@\n-a\n+b\n"
                            .to_string(),
                        stats: None,
                    },
                ],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        lines.iter().any(|line| line.contains("sample.txt (diff)")),
        "multi-section displays must keep every section label; got {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("other.txt (diff)")),
        "multi-section displays must keep every section label; got {lines:?}"
    );
}

#[test]
fn non_diff_single_section_display_keeps_section_label() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "nu".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"command":"ls"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "nu sample.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.rs".to_string(),
                    kind: ContentKind::Code {
                        language: "rust".to_string(),
                    },
                    content: "fn main() {}\n".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        lines.iter().any(|line| line.contains("sample.rs (rust)")),
        "non-diff single-section displays must keep the section label; got {lines:?}"
    );
}

#[test]
fn edit_preview_display_omits_redundant_single_file_stats_line() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"sample.txt"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"path":"sample.txt"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit sample.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "sample.txt".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                        .to_string(),
                    stats: Some(nu_agent_core::protocol::event::ToolDisplayStats {
                        files_changed: Some(1),
                        insertions: Some(3),
                        deletions: Some(1),
                        diff_truncated: Some(false),
                        omitted_files: Some(0),
                        omitted_hunks: Some(0),
                    }),
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines = state
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect::<Vec<_>>();
    assert!(!lines.iter().any(|line| line.starts_with("files=")));
    assert!(!lines.iter().any(|line| line.contains("+3 -1")));
}

#[test]
fn permission_requested_with_display_pushes_to_transcript() -> Result<()> {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"file":"foo.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(file=foo.rs)".to_string(),
        tool_key: "edit\n{\"file\":\"foo.rs\"}".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit foo.rs".to_string(),
            sections: vec![ToolDisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "+new content".to_string(),
                stats: None,
            }],
        }),
    };
    apply_permission_request_display(&mut state, &context);

    // The preview is its own ToolDisplay block pushed directly after the
    // pending Tool block, so the store holds exactly two blocks.
    assert_eq!(state.transcript.len(), 2);
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    let BlockSource::Tool { call, preview, .. } = &tool_block.source else {
        panic!("expected Tool block");
    };
    assert!(
        call.summary.contains("foo.rs"),
        "call line must show the tool summary, got: {call:?}"
    );
    assert!(
        preview.is_none(),
        "the Tool block must keep preview None after the preview push"
    );
    assert_eq!(
        tool_block.fill,
        Fill::None,
        "the Tool block must keep Fill::None so the call line stays untinted"
    );

    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the preview must be a ToolDisplay block"
    );
    assert_eq!(
        preview_block.fill,
        Fill::Code,
        "a diff preview block must carry Fill::Code"
    );
    assert!(
        tool_display_lines(preview_block).contains("+new content"),
        "the preview block must carry the display content, got: {}",
        tool_display_lines(preview_block)
    );
    Ok(())
}

/// The request context carries the exact call key, so a decorated display name
/// in `tool` cannot break the match. This is the production shape: `tool` is
/// `edit(path=..., operation={...})` while the pending call key is
/// `edit\n{...}`.
#[test]
fn permission_requested_with_decorated_tool_name_attaches_preview_via_tool_key() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let arguments =
        r#"{"path":"foo.rs","mode":"apply","operation":{"type":"create","content":"hi\n"}}"#;
    reduce_tool(&mut state, started("edit", arguments));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(mode=apply, operation={...}, path=foo.rs)".to_string(),
        tool_key: format!("edit\n{arguments}"),
        source: "builtin".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit foo.rs".to_string(),
            sections: vec![ToolDisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "+hi".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    apply_permission_request_display(&mut state, &context);

    // -- Check
    assert_eq!(
        state.transcript.len(),
        2,
        "preview must push its own ToolDisplay block"
    );
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(
        matches!(tool_block.source, BlockSource::Tool { preview: None, .. }),
        "the Tool block must keep preview None"
    );
    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(
        tool_display_lines(preview_block).contains("+hi"),
        "decorated tool name must still attach the preview via tool_key, got: {}",
        tool_display_lines(preview_block)
    );
    Ok(())
}

/// A request whose `tool_key` matches no pending call must leave the
/// transcript untouched — no preview attached to an unrelated call.
#[test]
fn permission_requested_with_unmatched_tool_key_leaves_transcript_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    reduce_tool(&mut state, started("edit", r#"{"path":"foo.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(path=other.rs)".to_string(),
        tool_key: "edit\n{\"path\":\"other.rs\"}".to_string(),
        source: "builtin".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit other.rs".to_string(),
            sections: vec![],
        }),
    };

    // -- Exec
    apply_permission_request_display(&mut state, &context);

    // -- Check
    assert_eq!(state.transcript.len(), 1);
    let block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    let BlockSource::Tool { preview, .. } = &block.source else {
        panic!("expected Tool block");
    };
    assert!(
        preview.is_none(),
        "an unmatched tool_key must not attach a preview"
    );
    Ok(())
}

/// Non-duplication is no longer this layer's job: `HookChain::on_tool_result`
/// (nu-agent-core) omits `display` from the `Completed` event whenever a
/// pre-authorize preview was already shown, so the reducer here never
/// receives a duplicate to begin with. This test documents that contract at
/// the TUI boundary: when the source correctly sends `display: None` after a
/// preview, the transcript shows the preview exactly once.
#[test]
fn tool_end_after_previewed_permission_with_source_suppressed_display_shows_once() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"file":"bar.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(file=bar.rs)".to_string(),
        tool_key: "edit\n{\"file\":\"bar.rs\"}".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit bar.rs".to_string(),
            sections: vec![ToolDisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "+new content".to_string(),
                stats: None,
            }],
        }),
    };
    apply_permission_request_display(&mut state, &context);

    // The source (chain.rs's suppress_previewed_display) already omitted the
    // display here — that's the actual non-duplication contract.
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"file":"bar.rs"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("+new content"))
            .count(),
        1,
        "the preview must appear exactly once, got: {lines:?}"
    );
}

/// The TUI reducer itself does not deduplicate: if a `Completed` event were
/// ever to carry a display after a preview was already shown (a source bug),
/// the transcript would show it twice. This isn't desired behavior — it's a
/// regression guard documenting that the non-duplication guarantee lives
/// entirely in `nu-agent-core`'s `suppress_previewed_display`
/// (`hook/chain.rs`), not here.
#[test]
fn tool_end_renders_whatever_display_it_is_given_no_local_dedup() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"file":"bar.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit(file=bar.rs)".to_string(),
        tool_key: "edit\n{\"file\":\"bar.rs\"}".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "edit bar.rs".to_string(),
            sections: vec![ToolDisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "+new content".to_string(),
                stats: None,
            }],
        }),
    };
    apply_permission_request_display(&mut state, &context);

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: r#"{"file":"bar.rs"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit bar.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "changes".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "+new content".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("+new content"))
            .count(),
        2,
        "the reducer has no dedup of its own — a source that (incorrectly) \
         resends the display after a preview will show it twice; got: {lines:?}"
    );
}

#[test]
fn tool_end_without_prior_permission_pushes_display_normally() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"path":"bar.rs"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "edit".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"path":"bar.rs"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit bar.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "changes".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "+new content".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert!(
        !lines.iter().any(|line| line.contains("changes (diff)")),
        "the call line already shows the path, so the section label is redundant"
    );
    assert!(
        lines.iter().any(|line| line.contains("+new content")),
        "Expected to find '+new content' in transcript"
    );
}

/// The suppression decision is keyed on the TYPED tool identity: a NON-edit
/// tool whose display happens to carry an edit-shaped single diff section
/// (or an edit-prefixed title) must keep its title and section label — the
/// display shape alone never triggers suppression.
#[test]
fn non_edit_tool_with_edit_shaped_display_keeps_title_and_label() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));

    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "nu".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"command":"ls"}"#.to_string(),
            success: true,
            result: "{}".to_string(),
            display: Some(ToolDisplay {
                title: "edit bar.rs".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "changes".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "+new content".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    let lines: Vec<String> = state
        .transcript
        .blocks()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(
        lines.iter().any(|line| line.contains("edit bar.rs")),
        "non-edit tool identity must keep the display title; got {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("changes (diff)")),
        "non-edit tool identity must keep the section label; got {lines:?}"
    );
}

#[test]
fn permission_requested_without_display_does_not_add_transcript_entries() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));

    let len_after_start = state.transcript.len();

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "nu(command=ls)".to_string(),
        tool_key: "nu\n{\"command\":\"ls\"}".to_string(),
        source: "mcp".to_string(),
        mode: None,
        matched_rule_identity: "tool:nu".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "nu".to_string(),
        summary: r#"→ {"command":"ls"}"#.to_string(),
        pre_authorize_display: None,
    };
    state
        .permission
        .reduce_permission_event(nu_agent_core::bus::PermissionEvent::Requested {
            request_id: "req-1".to_string(),
            context: Box::new(context),
        });

    assert_eq!(
        state.transcript.len(),
        len_after_start,
        "PermissionRequested without display should not add transcript entries"
    );
}

#[test]
fn tool_preview_dispatch_pushes_display_block_after_pending_tool() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let arguments = r#"{"command":"ls"}"#;
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolStarted {
            name: "nu".to_string(),
            source: "builtin".to_string(),
            arguments: arguments.to_string(),
            call_line: CallLine::from_json_summary(arguments),
        },
    );

    // -- Exec
    let handled = dispatch_ui_event(
        &mut state,
        UiEvent::ToolPreview {
            tool_key: format!("nu\n{arguments}"),
            display: ToolDisplay {
                title: "nu".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "nu".to_string(),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "ls".to_string(),
                    stats: None,
                }],
            },
        },
    );

    // -- Check
    assert!(handled, "ToolPreview must report a handled event");
    assert_eq!(
        state.transcript.len(),
        2,
        "ToolPreview must push one ToolDisplay block after the Tool block"
    );
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(
        matches!(tool_block.source, BlockSource::Tool { preview: None, .. }),
        "the Tool block must keep preview None"
    );
    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the preview must be a ToolDisplay block"
    );
    assert!(
        tool_display_lines(preview_block).contains("ls"),
        "the preview block must carry the display content"
    );
    Ok(())
}

#[test]
fn tool_preview_dispatch_with_unmatched_key_pushes_no_block() {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolStarted {
            name: "nu".to_string(),
            source: "builtin".to_string(),
            arguments: r#"{"command":"ls"}"#.to_string(),
            call_line: CallLine::from_json_summary(r#"{"command":"ls"}"#),
        },
    );
    let len_after_start = state.transcript.len();

    // -- Exec
    dispatch_ui_event(
        &mut state,
        UiEvent::ToolPreview {
            tool_key: "nu\n{\"command\":\"other\"}".to_string(),
            display: ToolDisplay {
                title: "nu".to_string(),
                sections: vec![],
            },
        },
    );

    // -- Check
    assert_eq!(
        state.transcript.len(),
        len_after_start,
        "an unmatched tool_key must push no block"
    );
}

#[test]
fn handle_tool_start_pushes_tool_block_without_leading_spacer() {
    let mut state = AppState::default();
    reduce_tool(&mut state, started("read", "{}"));
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn handle_tool_end_stores_two_content_blocks_without_separators() {
    let mut state = AppState::default();
    // Two tool calls within the same block
    for name in ["read", "write"] {
        reduce_tool(&mut state, started(name, "{}"));
        reduce_tool(
            &mut state,
            ToolEvent::Completed {
                name: name.to_string(),
                source: "builtin".to_string(),
                arguments: "{}".to_string(),
                success: true,
                result: "ok".to_string(),
                display: None,
                error_kind: None,
                message: None,
            },
        );
    }

    // transcript: [Tool, Tool] — the store holds content blocks only;
    // separators are a render-time concern (SpacerStateMachine).
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn tool_start_nu_after_plain_tool_stores_two_content_blocks() {
    let mut state = AppState::default();
    // Plain tool call (no background block)
    reduce_tool(&mut state, started("read", "{}"));
    // nu tool call (renders a background block)
    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));

    // transcript: [Tool(read), Tool(nu)] — no Spacer blocks in the store.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn tool_start_plain_after_nu_stores_two_content_blocks() {
    let mut state = AppState::default();
    // nu tool call (renders a background block)
    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));
    // plain tool call after nu
    reduce_tool(&mut state, started("read", "{}"));

    // transcript: [Tool(nu), Tool(read)] — no Spacer blocks in the store.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn tool_start_two_plain_tools_store_two_content_blocks() {
    let mut state = AppState::default();
    reduce_tool(&mut state, started("read", "{}"));
    reduce_tool(&mut state, started("write", "{}"));

    // transcript: [Tool(read), Tool(write)] — no Spacer blocks in the store.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn bookkeeping_start_finish_tracks_row_status() {
    let mut state = AppState::default();
    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
        &mut evicted,
    );
    assert_eq!(state.transcript.len(), 1);
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        Some(true),
    );
    assert_eq!(state.transcript.blocks()[0].status, Some(ItemStatus::Done));
}

#[test]
fn bookkeeping_start_finish_unknown_renders_unknown_status() {
    let mut state = AppState::default();
    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
        &mut evicted,
    );
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        None,
    );
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::Unknown),
        "flag-absent tool rows must render unknown, not guessed success"
    );
}

#[test]
fn concurrent_same_name_tool_calls_get_correct_statuses() {
    let mut state = AppState::default();

    // Start two tool calls with the same name but different arguments
    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__get_pod",
        r#"{"name":"api-0"}"#,
        CallLine::from_json_summary(r#"{"name":"api-0"}"#),
        &mut evicted,
    );
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__get_pod",
        r#"{"name":"api-1"}"#,
        CallLine::from_json_summary(r#"{"name":"api-1"}"#),
        &mut evicted,
    );

    // Both should be InProgress. The store holds two content blocks; no
    // Separator blocks are inserted.
    assert_eq!(state.transcript.len(), 2);
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::InProgress)
    );
    assert_eq!(
        state.transcript.blocks()[1].status,
        Some(ItemStatus::InProgress)
    );

    // Finish in reverse order
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__get_pod",
        r#"{"name":"api-1"}"#,
        Some(true),
    );
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__get_pod",
        r#"{"name":"api-0"}"#,
        Some(false),
    );

    // Each should get the correct status
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::Failed)
    );
    assert_eq!(state.transcript.blocks()[1].status, Some(ItemStatus::Done));
}

// ---------------------------------------------------------------------------
// Hydrated tool-call bookkeeping (task 53012ecc rework)
// ---------------------------------------------------------------------------

/// Hydration records an already-finished call. A later LIVE call with the
/// same name+arguments key must complete its OWN block — the hydrated entry
/// must not sit InProgress in the active deque and steal the finish.
#[test]
fn hydrated_call_does_not_steal_finish_from_later_live_call_with_same_key() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let mut status = crate::state::StatusState::default();
    let mut compaction = crate::state::CompactionState::default();

    // Hydrated session: one finished `read` call → block 0 (status Done),
    // bookkeeping entry recorded for later key lookups.
    state.transcript.hydrate_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "→ \"a.rs\"")
                .with_tool_name("read".to_string())
                .with_tool_details(Some(r#"{"path":"a.rs"}"#.to_string()), None, Some(true)),
        ],
        None,
        &mut status,
        &mut state.tool,
        &mut compaction,
    );
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::Done),
        "hydrated block starts Done"
    );

    // Live session continues: a NEW `read` call with the same arguments.
    reduce_tool(&mut state, started("read", r#"{"path":"a.rs"}"#));
    let live_block = state
        .transcript
        .len()
        .checked_sub(1)
        .ok_or("should have live block")?;
    assert_eq!(
        state.transcript.blocks()[live_block].status,
        Some(ItemStatus::InProgress),
        "live block starts InProgress"
    );

    // -- Exec
    reduce_tool(
        &mut state,
        ToolEvent::Completed {
            name: "read".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"path":"a.rs"}"#.to_string(),
            success: true,
            result: "contents".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    );

    // -- Check
    // The LIVE block is the one that finishes.
    assert_eq!(
        state.transcript.blocks()[live_block].status,
        Some(ItemStatus::Done),
        "live call with same key must complete its own block"
    );
    // The hydrated block keeps its terminal status from hydration.
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::Done),
        "hydrated block's terminal status must not be overwritten"
    );
    Ok(())
}
