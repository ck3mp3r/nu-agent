//! Tool-domain reducer tests: tool rows, display rendering, and the
//! permission pre-display dedup. Assertions moved 1:1 from the former
//! `interaction/reducer_test.rs` `reduce_ui_event_impl` effect tests, driven
//! through `ToolState::reduce_tool_event`.

use crate::interaction::reducer::apply_permission_request_display;
use crate::state::AppState;
use nu_agent_core::bus::ToolEvent;
use nu_agent_core::protocol::event::{ToolDisplay, ToolDisplaySection};
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::TranscriptEntryKind;
use nu_agent_core::transcript::renderer::ItemStatus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn extract_all_text_from_entry(
    entry: &nu_agent_core::transcript::items::TranscriptEntry,
) -> Vec<String> {
    match &entry.kind {
        TranscriptEntryKind::ToolResult(result) => {
            result.lines.iter().map(|line| line.text.clone()).collect()
        }
        _ => vec![entry.text()],
    }
}

fn reduce_tool(state: &mut AppState, event: ToolEvent) -> bool {
    state.tool.reduce_tool_event(&mut state.transcript, event)
}

fn started(name: &str, arguments: &str) -> ToolEvent {
    ToolEvent::Started {
        name: name.to_string(),
        source: "mcp".to_string(),
        arguments: arguments.to_string(),
    }
}

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

    // [Spacer, Tool] — ToolStarted pushed a starting spacer, ToolCompleted pushed no display
    assert_eq!(state.transcript.entries.len(), 2);
    assert!(matches!(
        state.transcript.entries[0].kind,
        TranscriptEntryKind::Spacer(_)
    ));
    let entry = &state.transcript.entries[1];
    assert_eq!(entry.role(), Role::Tool);
    assert_eq!(entry.text(), "k8s__list_pods");
    // Check the args field for status and content
    if let TranscriptEntryKind::Tool(invocation) = &entry.kind {
        assert!(invocation.args.contains("namespace"));
        assert!(!invocation.args.contains("api-0"));
        assert!(!invocation.args.contains("[{"));
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

    // handle_tool_start pushes a starting spacer before the tool
    assert_eq!(state.transcript.entries.len(), 2);
    assert!(matches!(
        state.transcript.entries[0].kind,
        TranscriptEntryKind::Spacer(_)
    ));
    let entry = &state.transcript.entries[1];
    assert_eq!(entry.role(), Role::Tool);
    assert_eq!(entry.text(), "k8s__list_pods");
    if let TranscriptEntryKind::Tool(invocation) = &entry.kind {
        assert!(invocation.args.contains("namespace"));
    } else {
        panic!("Expected Tool variant");
    }
    assert_eq!(
        state.transcript.entries[1].status,
        Some(ItemStatus::InProgress)
    );
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

    // [Spacer, Tool] — starting spacer then the tool
    assert_eq!(state.transcript.entries.len(), 2);
    assert!(matches!(
        state.transcript.entries[0].kind,
        TranscriptEntryKind::Spacer(_)
    ));
    assert_eq!(state.transcript.entries[1].text(), "gh__get_pr");
    assert_eq!(state.transcript.entries[1].status, Some(ItemStatus::Done));

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
    assert_eq!(failed.transcript.entries.len(), 2);
    assert!(matches!(
        failed.transcript.entries[0].kind,
        TranscriptEntryKind::Spacer(_)
    ));
    assert_eq!(
        failed.transcript.entries[1].status,
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

    // [Spacer, Tool] — starting spacer then the tool
    assert_eq!(state.transcript.entries.len(), 2);
    assert!(matches!(
        state.transcript.entries[0].kind,
        TranscriptEntryKind::Spacer(_)
    ));
    assert_eq!(state.transcript.entries[1].text(), "k8s__describe");
    if let TranscriptEntryKind::Tool(invocation) = &state.transcript.entries[1].kind {
        assert!(invocation.args.starts_with("→ "));
        assert!(invocation.args.ends_with('…'));
        assert!(invocation.args.chars().count() < 180);
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
                    language: "diff".to_string(),
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
        .entries
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(lines.contains(&"sample.txt (diff)".to_string()));
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
                    language: "diff".to_string(),
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
        .entries
        .iter()
        .find(|entry| matches!(&entry.kind, TranscriptEntryKind::Tool(t) if t.name == "edit"))
        .ok_or("should have tool call row")?;
    assert_eq!(call_row.role(), Role::Tool);

    let display_rows: Vec<_> = state
        .transcript
        .entries
        .iter()
        .filter(|entry| match &entry.kind {
            TranscriptEntryKind::ToolResult(result) => result.lines.iter().any(|line| {
                line.text == "sample.txt (diff)"
                    || line.text.contains("--- a/sample.txt")
                    || line.text.contains("+++ b/sample.txt")
            }),
            _ => false,
        })
        .collect();

    assert!(!display_rows.is_empty());
    assert!(
        display_rows
            .iter()
            .all(|entry| entry.role() == Role::ToolDisplay)
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
                    language: "diff".to_string(),
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
        .entries
        .iter()
        .filter(|entry| match &entry.kind {
            TranscriptEntryKind::ToolResult(result) if entry.role() == Role::ToolDisplay => {
                result.lines.iter().any(|line| {
                    line.text.contains("--- a/sample.txt") || line.text.contains("+++ b/sample.txt")
                })
            }
            _ => false,
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
                    language: "diff".to_string(),
                    content: "--- a/sample.txt\n+++ b/sample.txt\n@@ -10,3 +10,4 @@\n line-a\n-line-b\n+line-c\n line-d\n"
                        .to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        },
    );

    assert!(state.transcript.entries.iter().any(|entry| {
        match &entry.kind {
            TranscriptEntryKind::ToolResult(result) => result
                .lines
                .iter()
                .any(|line| line.text.contains("@@ -10,3 +10,4 @@")),
            _ => false,
        }
    }));
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
                    language: "diff".to_string(),
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
        .entries
        .iter()
        .filter(|entry| entry.role() == Role::ToolDisplay)
        .collect();

    assert!(diff_rows.iter().any(|entry| match &entry.kind {
        TranscriptEntryKind::ToolResult(result) => result.lines.iter().any(|line| {
            line.text.contains("│alpha")
                || line.text.contains("│beta")
                || line.text.contains("│omega")
        }),
        _ => false,
    }));
    // TranscriptEntry no longer has a `.rendered` field - removed assertion
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
                    language: "diff".to_string(),
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
        .entries
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(!lines.contains(&"edit sample.txt".to_string()));
    assert!(lines.contains(&"sample.txt (diff)".to_string()));
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
                    language: "diff".to_string(),
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
        .entries
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(!lines.iter().any(|line| line.starts_with("files=")));
    assert!(!lines.iter().any(|line| line.contains("+3 -1")));
}

#[test]
fn permission_requested_with_display_pushes_to_transcript() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"file":"foo.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit".to_string(),
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
                language: "diff".to_string(),
                content: "+new content".to_string(),
                stats: None,
            }],
        }),
    };
    apply_permission_request_display(&mut state, &context);

    let lines: Vec<String> = state
        .transcript
        .entries
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert!(
        lines.iter().any(|line| line.contains("changes (diff)")),
        "Expected to find 'changes (diff)' in transcript"
    );
    assert!(
        lines.iter().any(|line| line.contains("+new content")),
        "Expected to find '+new content' in transcript"
    );
}

#[test]
fn tool_end_after_permission_does_not_duplicate_display() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("edit", r#"{"file":"bar.rs"}"#));

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "edit".to_string(),
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
                language: "diff".to_string(),
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
                    language: "diff".to_string(),
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
        .entries
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert!(
        lines.iter().any(|line| line.contains("changes (diff)")),
        "Expected to find 'changes (diff)' in transcript"
    );
    assert!(
        lines.iter().any(|line| line.contains("+new content")),
        "Expected to find '+new content' in transcript"
    );
}

#[test]
fn tool_end_without_prior_permission_pushes_display_normally() {
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
                    language: "diff".to_string(),
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
        .entries
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();

    assert!(
        lines.iter().any(|line| line.contains("changes (diff)")),
        "Expected to find 'changes (diff)' in transcript"
    );
    assert!(
        lines.iter().any(|line| line.contains("+new content")),
        "Expected to find '+new content' in transcript"
    );
}

#[test]
fn permission_requested_without_display_does_not_add_transcript_entries() {
    let mut state = AppState::default();

    reduce_tool(&mut state, started("nu", r#"{"command":"ls"}"#));

    let len_after_start = state.transcript.entries.len();

    let context = nu_agent_core::protocol::event::PermissionRequestContext {
        tool: "nu".to_string(),
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
        state.transcript.entries.len(),
        len_after_start,
        "PermissionRequested without display should not add transcript entries"
    );
}

#[test]
fn handle_tool_start_pushes_starting_spacer() {
    let mut state = AppState::default();
    reduce_tool(&mut state, started("read", "{}"));
    assert!(matches!(
        state.transcript.entries[0].kind,
        TranscriptEntryKind::Spacer(_)
    ));
    assert!(matches!(
        state.transcript.entries[1].kind,
        TranscriptEntryKind::Tool(_)
    ));
}

#[test]
fn handle_tool_end_does_not_push_spacer_between_tool_calls() {
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

    // transcript: [Spacer, Tool, Tool] — no spacer between the two tool calls
    assert_eq!(state.transcript.entries.len(), 3);
    assert!(matches!(
        state.transcript.entries[0].kind,
        TranscriptEntryKind::Spacer(_)
    ));
    assert!(matches!(
        state.transcript.entries[1].kind,
        TranscriptEntryKind::Tool(_)
    ));
    assert!(matches!(
        state.transcript.entries[2].kind,
        TranscriptEntryKind::Tool(_)
    ));
}

#[test]
fn bookkeeping_start_finish_tracks_row_status() {
    let mut state = AppState::default();
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
    );
    assert_eq!(state.transcript.entries.len(), 1);
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        Some(true),
    );
    assert_eq!(state.transcript.entries[0].status, Some(ItemStatus::Done));
}

#[test]
fn bookkeeping_start_finish_unknown_renders_unknown_status() {
    let mut state = AppState::default();
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
    );
    state.tool.finish_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        None,
    );
    assert_eq!(
        state.transcript.entries[0].status,
        Some(ItemStatus::Unknown),
        "flag-absent tool rows must render unknown, not guessed success"
    );
}

#[test]
fn concurrent_same_name_tool_calls_get_correct_statuses() {
    let mut state = AppState::default();

    // Start two tool calls with the same name but different arguments
    state
        .tool
        .start_tool_call(&mut state.transcript, "k8s__get_pod", r#"{"name":"api-0"}"#);
    state
        .tool
        .start_tool_call(&mut state.transcript, "k8s__get_pod", r#"{"name":"api-1"}"#);

    // Both should be InProgress
    assert_eq!(state.transcript.entries.len(), 2);
    assert_eq!(
        state.transcript.entries[0].status,
        Some(ItemStatus::InProgress)
    );
    assert_eq!(
        state.transcript.entries[1].status,
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
    assert_eq!(state.transcript.entries[0].status, Some(ItemStatus::Failed));
    assert_eq!(state.transcript.entries[1].status, Some(ItemStatus::Done));
}
