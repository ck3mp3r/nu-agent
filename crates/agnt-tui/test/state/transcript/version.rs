use super::*;

/// WHEN an entry is pushed, THEN it SHALL carry its own cache identity, and a
/// later read of that entry SHALL return the same identity — a parked canvas
/// must survive every frame that changed nothing.
#[test]
fn test_transcript_store_entry_version_is_unique_and_stable() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    push_user(&mut state, "run it");
    push_assistant(&mut state, "first");

    // -- Exec
    let first = state.transcript.entry_version(0).ok_or("entry 0 exists")?;
    let second = state.transcript.entry_version(1).ok_or("entry 1 exists")?;
    let again = state.transcript.entry_version(0).ok_or("entry 0 exists")?;

    // -- Check: two entries never share an id, and a read changes nothing.
    assert_ne!(first.id, second.id, "each entry takes its own identity");
    assert_eq!(first, again, "a read is stable");
    assert_eq!(
        state.transcript.entry_version(2),
        None,
        "an entry the store never held carries no identity"
    );
    Ok(())
}

/// WHEN an entry's content changes in place, THEN its revision SHALL move and
/// its id SHALL stay: one entry, one parked canvas, replaced on the next paint.
#[test]
fn test_transcript_store_entry_version_moves_on_a_content_change() -> Result<()> {
    // -- Setup & Fixtures: a tool entry and a streamed assistant entry.
    let width = 60;
    let mut state = AppState::default();
    state.transcript.push_entry(simple_tool_entry());
    push_assistant(&mut state, "first");
    state.transcript.rebuild_height_index(width);
    let tool = state.transcript.entry_version(0).ok_or("entry 0 exists")?;
    let streamed = state.transcript.entry_version(1).ok_or("entry 1 exists")?;

    // -- Exec: the tool result lands, then a streamed chunk folds.
    assert!(
        state.transcript.set_tool_result_at(
            0,
            Display {
                title: None,
                sections: Vec::new(),
            }
        ),
        "the fixture's first entry must be a tool entry"
    );
    state
        .transcript
        .fold_stream_markdown_at(1, "first\nsecond".to_string());
    state.transcript.ensure_height_index(width);

    // -- Check: both revisions moved, both ids stayed.
    let tool_after = state.transcript.entry_version(0).ok_or("entry 0 exists")?;
    let streamed_after = state.transcript.entry_version(1).ok_or("entry 1 exists")?;
    assert_eq!(tool_after.id, tool.id, "the tool entry keeps its identity");
    assert!(
        tool_after.revision > tool.revision,
        "a new result moves the revision"
    );
    assert_eq!(
        streamed_after.id, streamed.id,
        "the streamed entry keeps its identity"
    );
    assert!(
        streamed_after.revision > streamed.revision,
        "a fold moves the revision"
    );
    Ok(())
}

/// WHEN a status is stamped on an entry behind the store's back, THEN the store
/// SHALL be told, so the parked rows of the old glyph are never served again.
#[test]
fn test_transcript_store_entry_version_moves_on_a_stamped_status() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.transcript.push_entry(simple_tool_entry());
    let before = state.transcript.entry_version(0).ok_or("entry 0 exists")?;

    // -- Exec
    state.transcript.bump_revision(0);

    // -- Check
    let after = state.transcript.entry_version(0).ok_or("entry 0 exists")?;
    assert_eq!(after.id, before.id, "the entry keeps its identity");
    assert!(
        after.revision > before.revision,
        "a stamped status moves the revision"
    );
    Ok(())
}

/// WHEN the cap evicts the oldest entries, THEN the survivors SHALL keep their
/// identities: eviction shifts indices and must not re-key anything.
#[test]
fn test_transcript_store_entry_version_follows_cap_eviction() -> Result<()> {
    // -- Setup & Fixtures: a full transcript whose last entry is the marker.
    let mut state = AppState::default();
    for row in 0..MAX_TRANSCRIPT_ENTRIES {
        push_user(&mut state, &format!("row {row}"));
    }
    push_assistant(&mut state, "the marker");
    let marker_index = MAX_TRANSCRIPT_ENTRIES - 1;
    let before = state
        .transcript
        .entry_version(marker_index)
        .ok_or("the marker entry exists")?;

    // -- Exec: one more push overflows the cap.
    push_user(&mut state, "next turn");

    // -- Check: the marker shifted down one index and kept its identity.
    let after = state
        .transcript
        .entry_version(marker_index - 1)
        .ok_or("the marker entry exists")?;
    assert_eq!(after, before, "eviction must not re-key a survivor");
    Ok(())
}

/// WHEN a session's transcript is cleared, THEN the entries pushed after it
/// SHALL carry fresh identities: a cleared store must not re-key a canvas the
/// cache still holds.
#[test]
fn test_transcript_store_entry_version_is_not_reused_after_a_clear() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    push_user(&mut state, "run it");
    let before = state.transcript.entry_version(0).ok_or("entry 0 exists")?;

    // -- Exec
    state.transcript.clear();
    push_user(&mut state, "a new session");

    // -- Check
    let after = state.transcript.entry_version(0).ok_or("entry 0 exists")?;
    assert!(
        after.id > before.id,
        "a cleared store must hand out a fresh identity, got {after:?} after {before:?}"
    );
    Ok(())
}
