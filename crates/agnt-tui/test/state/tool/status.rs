use super::*;

// ---------------------------------------------------------------------------
// Tool call lifecycle and status bookkeeping
// ---------------------------------------------------------------------------

/// The call line depends on the tool and the call's own arguments: `nu`
/// renders its command in the preview block and carries only its timeout,
/// other tools keep the arrow summary, and long summaries truncate.
#[test]
fn tool_start_call_line_summary_matrix() -> Result<()> {
    // -- Setup & Fixtures
    enum Expect {
        Timeout,
        Arrow,
        TruncatedArrow,
    }
    let long_args = format!("{{\"payload\":\"{}\"}}", "x".repeat(300));
    let cases: Vec<(&str, SystemEvent, Expect)> = vec![
        (
            "nu raw command",
            started("nu", r#"{"command":"ls | select name type size"}"#),
            Expect::Timeout,
        ),
        (
            "nu multi-line command",
            started(
                "nu",
                r#"{"command":"ls | where size > 1mb\n| select name type\n| sort-by modified"}"#,
            ),
            Expect::Timeout,
        ),
        (
            "nu without command key",
            started("nu", r#"{"timeout_seconds":5}"#),
            Expect::Arrow,
        ),
        (
            "non-nu tool",
            started("k8s__list_pods", r#"{"namespace":"prod"}"#),
            Expect::Arrow,
        ),
        (
            "long args truncate",
            started("k8s__describe", &long_args),
            Expect::TruncatedArrow,
        ),
    ];

    // -- Exec & Check
    for (label, event, expect) in cases {
        let mut state = AppState::default();
        reduce_ui(&mut state, event);
        let entry = tool_entry_at(&state, 0)?;
        let summary = call_line_of(entry);
        match expect {
            Expect::Timeout => assert_eq!(summary, "⏱ 120s", "{label}"),
            Expect::Arrow => assert!(summary.starts_with("→ "), "{label}: {summary}"),
            Expect::TruncatedArrow => {
                assert!(summary.starts_with("→ "), "{label}");
                assert!(summary.ends_with('…'), "{label}");
                assert!(summary.chars().count() < 180, "{label}");
            }
        }
    }
    Ok(())
}

/// Tool row lifecycle: a start materializes one Tool block with the args
/// summary and InProgress status; a Completed event transitions it to Done or
/// Failed and keeps the args summary without dumping the payload; the
/// low-level bookkeeping API records Done or Unknown; the status line stays
/// empty; and starts/ends store one content block each with no separators.
#[test]
fn tool_row_lifecycle_and_status_bookkeeping() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec: a tool start.
    reduce_ui(
        &mut state,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );

    // -- Check: one Tool entry, args summary, InProgress, empty status line.
    assert_eq!(state.transcript.len(), 1);
    let entry = tool_entry_at(&state, 0)?;
    assert!(call_line_of(entry).contains("namespace"));
    assert_eq!(entry.status, ItemStatus::InProgress);
    assert!(state.status.message.status_line().is_empty());

    // -- Exec & Check: a Completed event maps the row by the status variant.
    let cases: [(&str, ToolStatus, ItemStatus); 3] = [
        ("success", ToolStatus::Success, ItemStatus::Done),
        ("failed", ToolStatus::Failed, ItemStatus::Failed),
        ("cancelled", ToolStatus::Cancelled, ItemStatus::Cancelled),
    ];
    for (label, status, expected) in cases {
        let mut completed = AppState::default();
        reduce_ui(&mut completed, started("gh__get_pr", r#"{"number":1}"#));
        reduce_ui(
            &mut completed,
            completed_with(
                "gh__get_pr",
                agnt_core::tools::handler::ToolSource::Mcp,
                r#"{"number":1}"#,
                ToolOutcome {
                    success: matches!(status, ToolStatus::Success),
                    status,
                    result: "ok".to_string(),
                    error_kind: None,
                    message: None,
                },
            ),
        );
        assert_eq!(completed.transcript.len(), 1, "{label}");
        assert_eq!(tool_entry_at(&completed, 0)?.status, expected, "{label}");
    }

    // -- Exec & Check: the bookkeeping API records Done or Unknown.
    let bookkeeping: [(&str, Option<ToolStatus>, ItemStatus); 2] = [
        ("done", Some(ToolStatus::Success), ItemStatus::Done),
        ("unknown", None, ItemStatus::Unknown),
    ];
    for (label, status, expected) in bookkeeping {
        let mut state = AppState::default();
        state.tool.start_tool_call(
            &mut state.transcript,
            &tool_call("k8s__list_pods", r#"{"namespace":"prod"}"#),
        );
        state.tool.finish_tool_call(
            &mut state.transcript,
            &ToolCallId::new("k8s__list_pods", r#"{"namespace":"prod"}"#),
            status,
        );
        assert_eq!(state.transcript.len(), 1, "{label}");
        assert_eq!(tool_entry_at(&state, 0)?.status, expected, "{label}");
    }

    // -- Exec & Check: a Completed event keeps the args summary without the payload.
    let mut summary = AppState::default();
    reduce_ui(
        &mut summary,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );
    reduce_ui(
        &mut summary,
        completed_with(
            "k8s__list_pods",
            agnt_core::tools::handler::ToolSource::Mcp,
            r#"{"namespace":"prod"}"#,
            ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: r#"[{"name":"api-0","ns":"prod"}]"#.to_string(),
                error_kind: None,
                message: None,
            },
        ),
    );
    assert_eq!(summary.transcript.len(), 1);
    let entry = tool_entry_at(&summary, 0)?;
    let summary_text = call_line_of(entry);
    assert!(summary_text.contains("namespace"));
    assert!(!summary_text.contains("api-0"));
    assert!(!summary_text.contains("[{"));
    assert!(summary.status.message.status_line().is_empty());

    // -- Exec & Check: starts and ends store one content block each, no separators.
    let mixes: [(&str, &[&str]); 3] = [
        ("two plain tools", &["read", "write"]),
        ("nu after plain", &["read", "nu"]),
        ("plain after nu", &["nu", "read"]),
    ];
    for (label, names) in mixes {
        let mut state = AppState::default();
        for name in names {
            reduce_ui(&mut state, started(name, "{}"));
        }
        assert_eq!(state.transcript.len(), names.len(), "{label}");
        for entry in state.transcript.entries() {
            assert!(matches!(entry, TranscriptEntry::Tool(_)), "{label}");
        }
    }
    let mut state = AppState::default();
    for name in ["read", "write"] {
        reduce_ui(&mut state, started(name, "{}"));
        reduce_ui(
            &mut state,
            completed_with(
                name,
                agnt_core::tools::handler::ToolSource::Builtin,
                "{}",
                ToolOutcome {
                    success: true,
                    status: ToolStatus::Success,
                    result: "ok".to_string(),
                    error_kind: None,
                    message: None,
                },
            ),
        );
    }
    assert_eq!(state.transcript.len(), 2);
    for entry in state.transcript.entries() {
        assert!(matches!(entry, TranscriptEntry::Tool(_)));
    }
    Ok(())
}

#[test]
fn concurrent_same_name_tool_calls_get_correct_statuses() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    state.tool.start_tool_call(
        &mut state.transcript,
        &tool_call("k8s__get_pod", r#"{"name":"api-0"}"#),
    );
    state.tool.start_tool_call(
        &mut state.transcript,
        &tool_call("k8s__get_pod", r#"{"name":"api-1"}"#),
    );

    // -- Check
    assert_eq!(state.transcript.len(), 2);
    assert_eq!(tool_entry_at(&state, 0)?.status, ItemStatus::InProgress);
    assert_eq!(tool_entry_at(&state, 1)?.status, ItemStatus::InProgress);

    // Finish out of order: each call gets its own status.
    state.tool.finish_tool_call(
        &mut state.transcript,
        &ToolCallId::new("k8s__get_pod", r#"{"name":"api-1"}"#),
        Some(ToolStatus::Success),
    );
    state.tool.finish_tool_call(
        &mut state.transcript,
        &ToolCallId::new("k8s__get_pod", r#"{"name":"api-0"}"#),
        Some(ToolStatus::Failed),
    );
    assert_eq!(tool_entry_at(&state, 0)?.status, ItemStatus::Failed);
    assert_eq!(tool_entry_at(&state, 1)?.status, ItemStatus::Done);
    Ok(())
}

/// A cancelled completion renders the glyph only: the tool block carries the
/// Cancelled status and no content block is added, whatever the reason text
/// says. A failed completion's message stays unrendered too.
#[test]
fn cancelled_completed_renders_glyph_only() -> Result<()> {
    // -- Setup & Fixtures: the two reasons one cancelled class carries (a
    // stopped turn's call, a policy denial). The reason is content.
    let cases = [
        "Interrupted: the turn stopped before this call ran",
        "Permission denied: test rule",
    ];

    // -- Exec & Check: every cancelled reason leaves the tool block alone.
    for reason in cases {
        let mut state = AppState::default();
        reduce_ui(&mut state, started("read", r#"{"path":"/tmp/f"}"#));
        reduce_ui(
            &mut state,
            completed_with(
                "read",
                agnt_core::tools::handler::ToolSource::Builtin,
                r#"{"path":"/tmp/f"}"#,
                ToolOutcome {
                    success: false,
                    status: ToolStatus::Cancelled,
                    result: String::new(),
                    error_kind: None,
                    message: Some(reason.to_string()),
                },
            ),
        );

        assert_eq!(state.transcript.len(), 1, "{reason}");
        assert_eq!(
            tool_entry_at(&state, 0)?.status,
            ItemStatus::Cancelled,
            "{reason}"
        );
    }

    // -- Exec & Check: a failed completion's message is not rendered.
    let mut state = AppState::default();
    reduce_ui(&mut state, started("read", r#"{"path":"/tmp/f"}"#));
    reduce_ui(
        &mut state,
        completed_with(
            "read",
            agnt_core::tools::handler::ToolSource::Builtin,
            r#"{"path":"/tmp/f"}"#,
            ToolOutcome {
                success: false,
                status: ToolStatus::Failed,
                result: String::new(),
                error_kind: Some("tool_error".to_string()),
                message: Some("boom".to_string()),
            },
        ),
    );
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(tool_entry_at(&state, 0)?.status, ItemStatus::Failed);
    Ok(())
}
