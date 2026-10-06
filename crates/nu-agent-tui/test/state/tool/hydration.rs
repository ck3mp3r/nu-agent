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
