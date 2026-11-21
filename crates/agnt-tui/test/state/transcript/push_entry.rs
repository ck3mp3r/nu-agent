use super::*;

// ---------------------------------------------------------------------------
// Store holds content blocks only — separators are a render-time concern
// (task 4609782e). The store never inserts Spacer blocks; the number of
// visual separator rows between two blocks is decided by the render
// layer's planner (see agnt-render's planner tests).
// ---------------------------------------------------------------------------

/// Pushing entries stores one content entry each, in order, with no Spacer
/// entries, whatever the mix of user, assistant, tool, notice, and display
/// entries.
#[test]
fn push_entry_stores_content_entries_without_spacers() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check: the first entry is a user message.
    push_user(&mut state, "prompt one");
    assert_eq!(state.transcript.len(), 1);
    assert!(matches!(
        state.transcript.entries()[0],
        TranscriptEntry::Message(MessageEntry {
            role: MessageRole::User,
            ..
        })
    ));

    // -- Exec & Check: two user entries.
    let mut two_user = AppState::default();
    push_user(&mut two_user, "prompt one");
    push_user(&mut two_user, "prompt two");
    assert_eq!(two_user.transcript.len(), 2);
    assert!(matches!(
        two_user.transcript.entries()[1],
        TranscriptEntry::Message(MessageEntry {
            role: MessageRole::User,
            ..
        })
    ));

    // -- Exec & Check: user then assistant.
    let mut user_assistant = AppState::default();
    push_user(&mut user_assistant, "prompt one");
    push_assistant(&mut user_assistant, "response one");
    assert_eq!(user_assistant.transcript.len(), 2);
    assert!(matches!(
        user_assistant.transcript.entries()[1],
        TranscriptEntry::Message(MessageEntry {
            role: MessageRole::Assistant,
            ..
        })
    ));

    // -- Exec & Check: assistant then user.
    let mut assistant_user = AppState::default();
    push_assistant(&mut assistant_user, "response one");
    push_user(&mut assistant_user, "prompt two");
    assert_eq!(assistant_user.transcript.len(), 2);
    assert!(matches!(
        assistant_user.transcript.entries()[0],
        TranscriptEntry::Message(MessageEntry {
            role: MessageRole::Assistant,
            ..
        })
    ));

    // -- Exec & Check: tool then assistant.
    let mut tool_assistant = AppState::default();
    tool_assistant.transcript.push_entry(simple_tool_entry());
    push_assistant(&mut tool_assistant, "response one");
    assert_eq!(tool_assistant.transcript.len(), 2);
    assert!(matches!(
        tool_assistant.transcript.entries()[1],
        TranscriptEntry::Message(MessageEntry {
            role: MessageRole::Assistant,
            ..
        })
    ));

    // -- Exec & Check: three alternating entries.
    let mut three = AppState::default();
    push_user(&mut three, "prompt one");
    push_assistant(&mut three, "response one");
    push_user(&mut three, "prompt two");
    assert_eq!(three.transcript.len(), 3);
    for entry in three.transcript.entries() {
        assert!(matches!(entry, TranscriptEntry::Message(_)));
    }

    // -- Exec & Check: a task line stores content only.
    let mut mixed = AppState::default();
    mixed
        .transcript
        .push_entry(TranscriptEntry::Task(TaskEntry {
            name: "Compaction".to_string(),
            summary: "auto".to_string(),
            status: ItemStatus::InProgress,
        }));
    mixed.transcript.push_entry(simple_tool_entry());
    assert_eq!(mixed.transcript.len(), 2);
    assert!(matches!(
        mixed.transcript.entries()[1],
        TranscriptEntry::Tool(_)
    ));

    // -- Exec & Check: a tool entry carries its status.
    let mut tool_state = AppState::default();
    tool_state.transcript.push_entry(simple_tool_entry());
    let entry = tool_state
        .transcript
        .entries()
        .first()
        .ok_or("should have entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    assert_eq!(tool.status, ItemStatus::InProgress);
    Ok(())
}

// ---------------------------------------------------------------------------
// Deferred measure — a push waits for the frame (task 58c9899b)
// ---------------------------------------------------------------------------

/// WHEN a fold pushes an entry while a height index is live, THE store SHALL
/// append a pending placeholder and SHALL NOT measure the entry inside the
/// fold: the widget projection is the frame's work.
#[test]
fn test_transcript_store_push_entry_defers_the_measure_to_the_frame() -> Result<()> {
    // -- Setup & Fixtures: a live index, settled by one frame.
    let width = 40;
    let mut state = AppState::default();
    push_user(&mut state, "run the edit");
    state.transcript.rebuild_height_index(width);
    state.transcript.ensure_height_index(width);
    let measured_before = state.transcript.remeasured_total();

    // -- Exec: a fold pushes a tall entry.
    push_assistant(&mut state, "line one\nline two\nline three");

    // -- Check: nothing was measured, and the entry's slot is in the index.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "a push must defer the widget measure to the frame"
    );
    assert!(
        state.transcript.height_index_valid_for(width),
        "a push appends the entry's slot, so the index stays valid"
    );
    let pending_rows = state.transcript.total_visual_rows();

    // -- Exec: the frame reads the index.
    state.transcript.ensure_height_index(width);

    // -- Check: one measure, and the geometry is the wholesale one.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before + 1,
        "the frame measures the pending entry once"
    );
    assert!(
        state.transcript.total_visual_rows() > pending_rows,
        "the placeholder must not carry the entry's content rows"
    );
    assert_geometry_matches_wholesale(&state, width)?;
    Ok(())
}

/// WHEN the frame reads the index and nothing is pending, THE store SHALL
/// measure nothing: one measure per pushed entry, never one per frame.
#[test]
fn test_transcript_store_pending_entry_is_measured_once_per_push() -> Result<()> {
    // -- Setup & Fixtures
    let width = 60;
    let mut state = AppState::default();
    state.transcript.rebuild_height_index(width);
    state.transcript.ensure_height_index(width);
    let measured_before = state.transcript.remeasured_total();

    // -- Exec: a push, then two frames.
    state.transcript.push_entry(simple_tool_entry());
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "the push defers its measure"
    );
    state.transcript.ensure_height_index(width);
    state.transcript.ensure_height_index(width);

    // -- Check: exactly one measure across both frames.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before + 1,
        "the pending entry is measured once, by the first frame"
    );
    Ok(())
}

/// WHILE entries are pending, THE store SHALL defer every mark to the frame: a
/// second push defers beside the first, and one frame measures both — the fold
/// never measures.
#[test]
fn test_transcript_store_push_entry_defers_the_earlier_pending_entry() -> Result<()> {
    // -- Setup & Fixtures
    let width = 60;
    let mut state = AppState::default();
    state.transcript.rebuild_height_index(width);
    state.transcript.ensure_height_index(width);
    let measured_before = state.transcript.remeasured_total();

    // -- Exec: two pushes with no frame between them.
    push_user(&mut state, "first turn");
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "the first push defers its measure"
    );
    push_assistant(&mut state, "second turn\nwith two lines");

    // -- Check: the second push deferred beside the first; both wait.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "a push beside a pending entry defers too"
    );
    state.transcript.ensure_height_index(width);
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before + 2,
        "the frame measures every pending entry once"
    );
    assert_geometry_matches_wholesale(&state, width)?;
    Ok(())
}

// -- Test Support

/// Assert the store's cached geometry equals a wholesale rebuild of its own
/// entries at `width`.
fn assert_geometry_matches_wholesale(state: &AppState, width: usize) -> Result<()> {
    let mut wholesale = state.transcript.clone();
    wholesale.rebuild_height_index(width);
    assert_eq!(
        state.transcript.total_visual_rows(),
        wholesale.total_visual_rows(),
        "the settled index must match a wholesale rebuild"
    );
    for index in 0..state.transcript.len() {
        assert_eq!(
            state.transcript.start_row_of(index),
            wholesale.start_row_of(index),
            "start row of entry {index} must match a wholesale rebuild"
        );
    }
    let len = state.transcript.len();
    assert_eq!(
        state.transcript.frame_plan(0, len, width),
        wholesale.frame_plan(0, len, width),
        "the frame plan must match a wholesale rebuild"
    );
    Ok(())
}

/// WHEN a push overflows the cap, THEN THE pending placeholder SHALL follow the
/// entry it names through the eviction shift, so the frame measures the pushed
/// entry and not its neighbour.
#[test]
fn test_transcript_store_pending_entry_follows_cap_eviction() -> Result<()> {
    // -- Setup & Fixtures: a full transcript with a settled index.
    let width = 64;
    let mut state = AppState::default();
    for row in 0..MAX_TRANSCRIPT_ENTRIES {
        push_user(&mut state, &format!("row {row}"));
    }
    state.transcript.rebuild_height_index(width);
    state.transcript.ensure_height_index(width);

    // -- Exec: one push over the cap evicts the oldest entry, and the pushed
    // entry waits for the frame at its shifted slot.
    push_assistant(&mut state, "streamed tail\nsecond line");
    assert_eq!(
        state.transcript.len(),
        MAX_TRANSCRIPT_ENTRIES,
        "the cap holds"
    );
    let pending_rows = state.transcript.total_visual_rows();
    state.transcript.ensure_height_index(width);

    // -- Check: the frame's content rows arrive, and the whole index — the
    // shifted slot's start rows and every planned rect — is the wholesale one.
    assert!(
        state.transcript.total_visual_rows() > pending_rows,
        "the pending entry's content rows arrive with the frame"
    );
    assert_geometry_matches_wholesale(&state, width)?;
    Ok(())
}

/// WHEN an eviction swallows a pending entry's index, THEN THE store SHALL drop
/// the pending mark with it: no frame measures an entry that no longer exists.
#[test]
fn test_transcript_store_evicted_pending_entry_leaves_no_measure() -> Result<()> {
    // -- Setup & Fixtures: a pending entry over a live index.
    let width = 60;
    let mut state = AppState::default();
    state.transcript.rebuild_height_index(width);
    state.transcript.ensure_height_index(width);
    let measured_before = state.transcript.remeasured_total();
    push_user(&mut state, "pending entry");

    // -- Exec: an eviction that covers the pending entry's index.
    state.transcript.shift_indices_after_eviction(1);
    state.transcript.ensure_height_index(width);

    // -- Check: nothing is measured for the dropped entry.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "an evicted pending entry must leave no measure behind"
    );
    Ok(())
}

/// WHEN no height index is live, THE push SHALL defer nothing — it measures
/// nothing either, exactly as before: the next rebuild measures everything.
#[test]
fn test_transcript_store_push_entry_without_a_live_index_defers_nothing() -> Result<()> {
    // -- Setup & Fixtures: a store whose index was never built.
    let width = 60;
    let mut state = AppState::default();
    let measured_before = state.transcript.remeasured_total();

    // -- Exec
    push_user(&mut state, "one");
    push_assistant(&mut state, "two\nthree");

    // -- Check: no measure ran and no index was invented.
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "a push before the index exists defers nothing"
    );
    assert!(
        !state.transcript.height_index_valid_for(width),
        "a push must not report an index it never built"
    );

    // -- Exec: the first frame builds the index for every entry.
    state.transcript.ensure_height_index(width);

    // -- Check: the index is live and matches a wholesale rebuild, and a
    // rebuild is not counted as a re-measure.
    assert!(state.transcript.height_index_valid_for(width));
    assert_eq!(
        state.transcript.remeasured_total(),
        measured_before,
        "a wholesale rebuild counts no single-entry re-measure"
    );
    assert_geometry_matches_wholesale(&state, width)?;
    Ok(())
}
