use super::*;

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

    // Hydrated session: one finished `read` call → block 0 (status Done),
    // bookkeeping entry recorded for later key lookups.
    hydrate_by_replay(
        &mut state,
        vec![
            UiMessageSnapshot::new("tool", "→ \"a.rs\"")
                .with_tool_name("read".to_string())
                .with_tool_details(Some(r#"{"path":"a.rs"}"#.to_string()), None, Some(true)),
        ],
        None,
    );
    assert_eq!(
        tool_entry_at(&state, 0)?.status,
        ItemStatus::Done,
        "hydrated entry starts Done"
    );

    // Live session continues: a NEW `read` call with the same arguments.
    reduce_ui(&mut state, started("read", r#"{"path":"a.rs"}"#));
    let live_block = state
        .transcript
        .len()
        .checked_sub(1)
        .ok_or("should have live block")?;
    assert_eq!(
        tool_entry_at(&state, live_block)?.status,
        ItemStatus::InProgress,
        "live entry starts InProgress"
    );

    // -- Exec
    reduce_ui(
        &mut state,
        completed_with(
            "read",
            "mcp",
            r#"{"path":"a.rs"}"#,
            ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: "contents".to_string(),
                display: None,
                error_kind: None,
                message: None,
            },
        ),
    );

    // -- Check
    // The LIVE block is the one that finishes.
    assert_eq!(
        tool_entry_at(&state, live_block)?.status,
        ItemStatus::Done,
        "live call with same key must complete its own entry"
    );
    // The hydrated entry keeps its terminal status from hydration.
    assert_eq!(
        tool_entry_at(&state, 0)?.status,
        ItemStatus::Done,
        "hydrated entry's terminal status must not be overwritten"
    );
    Ok(())
}

/// A rehydrated not-run row lands its persisted args display in the entry's
/// PREVIEW slot — the slot main's hydration used and the slot the tool widget
/// bands the code surface on — and stamps the ⊘ status displayless.
#[test]
fn hydrated_not_run_row_renders_cancelled_with_the_gate_preview() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    hydrate_by_replay(
        &mut state,
        vec![
            UiMessageSnapshot::new("tool", "→ ⏱ 120s")
                .with_tool_name("nu".to_string())
                .with_tool_details(
                    Some(r#"{"command":"ls | sort"}"#.to_string()),
                    None,
                    Some(false),
                )
                .with_tool_status(ToolStatus::Cancelled),
            UiMessageSnapshot::new("tool_display", String::new()).with_tool_display(ToolDisplay {
                title: None,
                sections: vec![ToolDisplaySection {
                    label: None,
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "ls | sort".to_string(),
                    stats: None,
                }],
            }),
        ],
        None,
    );

    // -- Check
    assert_eq!(
        state.transcript.len(),
        1,
        "the display lives on the tool entry"
    );
    let entry = tool_entry_at(&state, 0)?;
    assert_eq!(
        entry.status,
        ItemStatus::Cancelled,
        "a rehydrated not-run row renders the ⊘ status"
    );
    assert!(
        entry.preview.is_some(),
        "the persisted display is the gate preview main's hydration docked"
    );
    assert!(
        entry.result.is_none(),
        "a cancelled call has no completion body, so the entry stays banded"
    );
    let preview = entry.preview.as_ref().ok_or("should have preview")?;
    assert_eq!(
        display_lines(preview),
        "ls | sort",
        "the gate preview carries the command"
    );
    assert!(
        preview.has_code_or_diff(),
        "a code display renders on the code surface"
    );
    Ok(())
}

/// The cancelled census: every persisted shape the store records the cancelled
/// variant for (a denied result, an unanswered close, rig's no-result
/// placeholder) replays as ⊘ with its args preview in the preview slot. A call
/// with NO persisted result at all carries no verdict: the replay rule makes it
/// a failed completion, and it invents no preview.
#[test]
fn hydrated_cancelled_census_keeps_the_gate_preview_for_every_shape() -> Result<()> {
    // -- Setup & Fixtures: the three persisted cancelled shapes, as the resolver
    // reads them from the store.
    let shapes = [
        "Permission denied by policy",
        "[interrupted]",
        "[no result provided]",
    ];
    for text in shapes {
        let mut state = AppState::default();

        // -- Exec
        hydrate_by_replay(
            &mut state,
            vec![
                UiMessageSnapshot::new("tool", format!("→ {text}"))
                    .with_tool_name("nu".to_string())
                    .with_tool_details(
                        Some(r#"{"command":"ls | sort"}"#.to_string()),
                        None,
                        Some(false),
                    )
                    .with_tool_status(ToolStatus::Cancelled),
                UiMessageSnapshot::new("tool_display", String::new()).with_tool_display(
                    ToolDisplay {
                        title: None,
                        sections: vec![ToolDisplaySection {
                            label: None,
                            kind: ContentKind::Code {
                                language: "nu".to_string(),
                            },
                            content: "ls | sort".to_string(),
                            stats: None,
                        }],
                    },
                ),
            ],
            None,
        );

        // -- Check: one entry, ⊘, preview in the gate slot whatever the
        // persisted verdict flag says.
        let entry = tool_entry_at(&state, 0)?;
        assert_eq!(entry.status, ItemStatus::Cancelled, "shape {text}");
        assert!(entry.preview.is_some(), "shape {text}: the gate preview");
        assert!(entry.result.is_none(), "shape {text}: no completion body");
    }

    // -- Setup & Fixtures: a call nothing answered — no verdict, no display.
    let mut unanswered = AppState::default();

    // -- Exec
    hydrate_by_replay(
        &mut unanswered,
        vec![
            UiMessageSnapshot::new("tool", "→ ⏱ 120s")
                .with_tool_name("nu".to_string())
                .with_tool_details(Some(r#"{"command":"ls"}"#.to_string()), None, None),
        ],
        None,
    );

    // -- Check: the wave's replay rule, not the retired hydration-only Unknown:
    // a completion carries a verdict, and no display is invented.
    let entry = tool_entry_at(&unanswered, 0)?;
    assert_eq!(
        entry.status,
        ItemStatus::Failed,
        "no verdict is not a success"
    );
    assert!(entry.preview.is_none(), "nothing to show");
    assert!(entry.result.is_none(), "no completion body");
    Ok(())
}
