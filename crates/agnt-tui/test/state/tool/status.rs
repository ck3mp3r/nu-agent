use super::*;

// ---------------------------------------------------------------------------
// Tool call lifecycle and status bookkeeping
// ---------------------------------------------------------------------------

/// The call-line summary depends on the tool and the call line the producer
/// supplies: `nu` renders its command in the preview block (empty summary),
/// other tools keep the arrow summary, and long summaries truncate.
#[test]
fn tool_start_call_line_summary_matrix() -> Result<()> {
    // -- Setup & Fixtures
    enum Expect {
        Empty,
        Arrow,
        TruncatedArrow,
    }
    let long_args = format!("{{\"payload\":\"{}\"}}", "x".repeat(300));
    let cases: Vec<(&str, ToolEvent, Expect)> = vec![
        (
            "nu raw command",
            started_with_call_line(
                "nu",
                r#"{"command":"ls | select name type size"}"#,
                CallLine {
                    summary: String::new(),
                },
            ),
            Expect::Empty,
        ),
        (
            "nu multi-line command",
            started_with_call_line(
                "nu",
                r#"{"command":"ls | where size > 1mb\n| select name type\n| sort-by modified"}"#,
                CallLine {
                    summary: String::new(),
                },
            ),
            Expect::Empty,
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
        reduce_tool(&mut state, event);
        let BlockSource::Tool { call, .. } = &state.transcript.blocks()[0].source else {
            return Err(format!("expected Tool variant for {label}").into());
        };
        match expect {
            Expect::Empty => assert_eq!(call.summary, String::new(), "{label}"),
            Expect::Arrow => assert!(call.summary.starts_with("→ "), "{label}: {}", call.summary),
            Expect::TruncatedArrow => {
                assert!(call.summary.starts_with("→ "), "{label}");
                assert!(call.summary.ends_with('…'), "{label}");
                assert!(call.summary.chars().count() < 180, "{label}");
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
    reduce_tool(
        &mut state,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );

    // -- Check: one Tool block, args summary, InProgress, empty status line.
    assert_eq!(state.transcript.len(), 1);
    let block = &state.transcript.blocks()[0];
    let BlockSource::Tool { call, .. } = &block.source else {
        return Err("expected Tool variant".into());
    };
    assert!(call.summary.contains("namespace"));
    assert_eq!(block.status, Some(ItemStatus::InProgress));
    assert!(state.status.message.status_line().is_empty());

    // -- Exec & Check: a Completed event transitions the row to Done or Failed.
    let cases: [(&str, bool, ItemStatus); 2] = [
        ("done", true, ItemStatus::Done),
        ("failed", false, ItemStatus::Failed),
    ];
    for (label, success, expected) in cases {
        let mut completed = AppState::default();
        reduce_tool(&mut completed, started("gh__get_pr", r#"{"number":1}"#));
        reduce_tool(
            &mut completed,
            ToolEvent::Completed {
                name: "gh__get_pr".to_string(),
                source: "mcp".to_string(),
                arguments: r#"{"number":1}"#.to_string(),
                success,
                result: "ok".to_string(),
                display: None,
                error_kind: if success {
                    None
                } else {
                    Some("tool_error".to_string())
                },
                message: if success {
                    None
                } else {
                    Some("boom".to_string())
                },
            },
        );
        assert_eq!(completed.transcript.len(), 1, "{label}");
        assert_eq!(
            completed.transcript.blocks()[0].status,
            Some(expected),
            "{label}"
        );
    }

    // -- Exec & Check: the bookkeeping API records Done or Unknown.
    let bookkeeping: [(&str, Option<bool>, ItemStatus); 2] = [
        ("done", Some(true), ItemStatus::Done),
        ("unknown", None, ItemStatus::Unknown),
    ];
    for (label, success, expected) in bookkeeping {
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
            success,
        );
        assert_eq!(state.transcript.len(), 1, "{label}");
        assert_eq!(
            state.transcript.blocks()[0].status,
            Some(expected),
            "{label}"
        );
    }

    // -- Exec & Check: a Completed event keeps the args summary without the payload.
    let mut summary = AppState::default();
    reduce_tool(
        &mut summary,
        started("k8s__list_pods", r#"{"namespace":"prod"}"#),
    );
    reduce_tool(
        &mut summary,
        ToolEvent::Completed {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: r#"{"namespace":"prod"}"#.to_string(),
            success: true,
            result: r#"[{"name":"api-0","ns":"prod"}]"#.to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    );
    assert_eq!(summary.transcript.len(), 1);
    let BlockSource::Tool { call, .. } = &summary.transcript.blocks()[0].source else {
        return Err("expected Tool variant".into());
    };
    assert!(call.summary.contains("namespace"));
    assert!(!call.summary.contains("api-0"));
    assert!(!call.summary.contains("[{"));
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
            reduce_tool(&mut state, started(name, "{}"));
        }
        assert_eq!(state.transcript.len(), names.len(), "{label}");
        for block in state.transcript.blocks() {
            assert!(matches!(block.source, BlockSource::Tool { .. }), "{label}");
        }
    }
    let mut state = AppState::default();
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
    assert_eq!(state.transcript.len(), 2);
    for block in state.transcript.blocks() {
        assert!(matches!(block.source, BlockSource::Tool { .. }));
    }
    Ok(())
}

#[test]
fn concurrent_same_name_tool_calls_get_correct_statuses() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let mut evicted = 0usize;

    // -- Exec
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

    // -- Check
    assert_eq!(state.transcript.len(), 2);
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::InProgress)
    );
    assert_eq!(
        state.transcript.blocks()[1].status,
        Some(ItemStatus::InProgress)
    );

    // Finish out of order: each call gets its own status.
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
    assert_eq!(
        state.transcript.blocks()[0].status,
        Some(ItemStatus::Failed)
    );
    assert_eq!(state.transcript.blocks()[1].status, Some(ItemStatus::Done));
    Ok(())
}
