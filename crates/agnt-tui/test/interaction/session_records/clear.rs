//! The clear battery: what the `/new` fold leaves behind when a session ends.
//! The tool rows it drops must not be reachable from the fresh session, and the
//! store's lifetime bookkeeping must start over.

use super::session_records_support_test::*;

#[test]
fn test_interaction_session_records_clear_drops_tool_bookkeeping() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    fold_record(&mut state, tool_started(edit_call()));
    let first_id = state
        .tool
        .calls
        .first()
        .ok_or("should have recorded the pre-clear call")?
        .id;

    // -- Exec
    let changed = fold_record(&mut state, session_started(false));

    // -- Check
    assert!(changed, "the clear is a fold the surface handles");
    assert!(
        state.tool.calls.is_empty(),
        "the cleared session keeps no tool rows"
    );
    assert!(
        state.tool.active_ids_by_key.is_empty(),
        "the cleared session keeps no in-flight ids"
    );
    fold_record(&mut state, tool_started(edit_call()));
    let fresh_id = state
        .tool
        .calls
        .first()
        .ok_or("should have recorded the post-clear call")?
        .id;
    assert!(
        fresh_id > first_id,
        "the id counter outlives the clear, so a fresh call never reuses a dropped id"
    );
    Ok(())
}

#[test]
fn test_interaction_session_records_clear_same_key_completion_stamps_fresh_row() -> Result<()> {
    // -- Setup & Fixtures: an in-flight call, a session that evicted before the
    // clear, and a second in-flight call under the SAME key.
    let mut state = AppState::default();
    fold_record(&mut state, tool_started(edit_call()));
    push_filler_past_cap(&mut state);
    assert!(
        state.transcript.evicted_total() > 0,
        "the session must carry an eviction history into the clear"
    );
    fold_record(&mut state, tool_started(edit_call()));
    fold_record(&mut state, session_started(false));
    fold_record(&mut state, tool_started(edit_call()));

    // -- Exec
    let changed = fold_record(&mut state, tool_completed(edit_call(), edit_outcome()));

    // -- Check
    assert!(changed, "the completion is a fold the surface handles");
    let entries = state.transcript.entries();
    assert_eq!(
        entries.len(),
        2,
        "the banner and the fresh tool row are the whole transcript"
    );
    let Some(entry) = entries.get(1) else {
        return Err("expected the fresh tool row".into());
    };
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("the fresh row must be the tool entry".into());
    };
    assert_eq!(
        tool.status,
        ItemStatus::Done,
        "the completion stamps its own row, not the row a dropped call pointed at"
    );
    assert!(
        tool.result.is_some(),
        "the completion display lands on its own row"
    );
    Ok(())
}

#[test]
fn test_interaction_session_records_clear_resets_store_lifetime() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.transcript.assistant_stream_start = Some(3);
    state.transcript.summary_stream_start = Some(5);
    push_filler_past_cap(&mut state);
    assert!(
        state.transcript.evicted_total() > 0,
        "the session must have evicted before the clear"
    );

    // -- Exec
    let changed = fold_record(&mut state, session_started(false));

    // -- Check
    assert!(changed, "the clear is a fold the surface handles");
    assert_eq!(
        state.transcript.evicted_total(),
        0,
        "the clear ends the store's lifetime eviction count"
    );
    assert_eq!(
        state.transcript.assistant_stream_start, None,
        "no streaming cursor survives the clear"
    );
    assert_eq!(
        state.transcript.summary_stream_start, None,
        "no summary cursor survives the clear"
    );
    assert_eq!(
        state.transcript.len(),
        1,
        "the fresh session starts at exactly one banner"
    );
    Ok(())
}
