use super::*;

// ---------------------------------------------------------------------------
// In-place tool entry mutation (task 7ab65c6a)
// ---------------------------------------------------------------------------

/// The arguments of the edit calls these tests drive.
const EDIT_ARGUMENTS: &str = r#"{"path":"a.rs"}"#;

/// `start_tool_call` creates exactly one tool entry; `finish_tool_call` and
/// `set_tool_preview` mutate that entry in place without pushing a new one.
#[test]
fn start_finish_and_preview_mutate_one_entry_in_place() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    state
        .tool
        .start_tool_call(&mut state.transcript, &tool_call("read", "{}"));

    // -- Check: one entry only — no separate spacer, no separate display entry.
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(tool_entry_at(&state, 0)?.status, ItemStatus::InProgress);

    // -- Exec
    state.tool.finish_tool_call(
        &mut state.transcript,
        &ToolCallId::new("read", "{}"),
        Some(ToolStatus::Success),
    );

    // -- Check: finish must not push a new entry.
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(tool_entry_at(&state, 0)?.status, ItemStatus::Done);

    // -- Setup & Fixtures: a preview set with a valid height index.
    let mut preview_state = AppState::default();
    let width = 80usize;
    preview_state.tool.start_tool_call(
        &mut preview_state.transcript,
        &tool_call("edit", EDIT_ARGUMENTS),
    );
    preview_state.transcript.rebuild_height_index(width);
    let rows_before = preview_state.transcript.total_visual_rows();
    assert!(
        preview_state.transcript.height_index_valid_for(width),
        "precondition: index must be valid before the preview is set"
    );

    // -- Exec
    let preview = edit_preview();
    preview_state.tool.set_tool_preview(
        &mut preview_state.transcript,
        &ToolCallId::new("edit", EDIT_ARGUMENTS),
        &preview,
    );

    // -- Check: the preview lands on the entry and pushes nothing.
    assert_eq!(preview_state.transcript.len(), 1);
    let entry = tool_entry_at(&preview_state, 0)?;
    assert_eq!(entry.preview.as_ref(), Some(&preview));

    // -- Check: the in-place mutation defers its measure, so the index stays
    // valid and the frame's flush carries the preview rows.
    assert!(
        preview_state.transcript.height_index_valid_for(width),
        "an in-place preview mutation must keep the height index valid"
    );
    preview_state.transcript.ensure_height_index(width);
    assert!(
        preview_state.transcript.total_visual_rows() > rows_before,
        "the preview content must add visual rows"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Preview-path height index (task 69706655)
// ---------------------------------------------------------------------------

/// `set_tool_preview` mutates the tool entry in place and defers its measure to
/// the frame, so the index stays valid and the frame's flush agrees with a
/// wholesale rebuild — both when the tool entry is last and when another entry
/// follows it.
#[test]
fn set_tool_preview_defers_to_the_frame_that_matches_a_wholesale_index() -> Result<()> {
    // -- Setup & Fixtures: a tool call with a valid index at width 80.
    let width = 80usize;
    let mut state = AppState::default();
    state
        .tool
        .start_tool_call(&mut state.transcript, &tool_call("edit", EDIT_ARGUMENTS));
    state.transcript.rebuild_height_index(width);
    assert!(
        state.transcript.height_index_valid_for(width),
        "precondition: index must be valid before the preview is set"
    );

    // -- Exec: the preview lands on the tail entry.
    state.tool.set_tool_preview(
        &mut state.transcript,
        &ToolCallId::new("edit", EDIT_ARGUMENTS),
        &edit_preview(),
    );

    // -- Check: one entry, index still valid, and the frame's flush matches a
    // wholesale rebuild.
    assert_eq!(state.transcript.len(), 1);
    assert!(state.transcript.height_index_valid_for(width));
    state.transcript.ensure_height_index(width);
    assert_eq!(
        state.transcript.total_visual_rows(),
        wholesale_total_rows(&state, width),
        "the frame's flush must match a wholesale rebuild after a tail preview"
    );

    // -- Setup & Fixtures: a user entry after the tool entry.
    let mut mid = AppState::default();
    mid.tool
        .start_tool_call(&mut mid.transcript, &tool_call("edit", EDIT_ARGUMENTS));
    mid.transcript
        .push_entry(TranscriptEntry::Message(MessageEntry {
            role: MessageRole::User,
            markdown: "later turn".to_string(),
            timestamp_millis: None,
            model: None,
            agent: None,
        }));
    mid.transcript.rebuild_height_index(width);
    assert!(
        mid.transcript.height_index_valid_for(width),
        "precondition: index must be valid before the preview is set"
    );

    // -- Exec: the preview lands on the first entry, before the user entry.
    mid.tool.set_tool_preview(
        &mut mid.transcript,
        &ToolCallId::new("edit", EDIT_ARGUMENTS),
        &edit_preview(),
    );

    // -- Check: the mutation keeps the entry count, leaves the index valid, and
    // the frame's flush matches a wholesale rebuild.
    assert_eq!(mid.transcript.len(), 2);
    assert!(mid.transcript.height_index_valid_for(width));
    mid.transcript.ensure_height_index(width);
    assert_eq!(
        mid.transcript.total_visual_rows(),
        wholesale_total_rows(&mid, width),
        "the frame's flush must match a wholesale rebuild after a mid-list preview"
    );
    Ok(())
}

// -- Test Support

/// WHEN a card attaches through the tool bookkeeping, THE preview SHALL land on
/// the entry holding that call — the index-correctness pin.
#[test]
fn card_attach_lands_on_the_calls_own_entry() -> Result<()> {
    // -- Setup & Fixtures: a call in flight, then a user entry, so the call's
    // entry is not the tail.
    let width = 80usize;
    let mut state = AppState::default();
    state
        .tool
        .start_tool_call(&mut state.transcript, &tool_call("edit", EDIT_ARGUMENTS));
    state
        .transcript
        .push_entry(TranscriptEntry::Message(MessageEntry {
            role: MessageRole::User,
            markdown: "later turn".to_string(),
            timestamp_millis: None,
            model: None,
            agent: None,
        }));
    state.transcript.rebuild_height_index(width);

    // -- Exec: the card arrives by key, as a permission request carries it.
    let attached = state.tool.set_preview_for_key(
        &mut state.transcript,
        ToolCallId::new("edit", EDIT_ARGUMENTS).as_str(),
        &edit_preview(),
    );
    assert!(attached, "the in-flight call must take the preview");

    // -- Check: exactly one entry carries the preview and it is the tool entry,
    // not the user entry that follows it.
    let with_preview = state
        .transcript
        .entries()
        .iter()
        .filter(|entry| matches!(entry, TranscriptEntry::Tool(tool) if tool.preview.is_some()))
        .count();
    assert_eq!(with_preview, 1, "exactly one entry carries the preview");
    assert!(
        matches!(
            state.transcript.entries().last(),
            Some(TranscriptEntry::Message(_))
        ),
        "the user entry must stay untouched at the tail"
    );
    Ok(())
}

/// The diff preview `set_tool_preview` attaches in these tests.
fn edit_preview() -> Display {
    Display {
        title: Some("edit a.rs".to_string()),
        sections: vec![DisplaySection {
            label: Some("changes".to_string()),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path: "test.rs".to_string(),
            },
            content: "--- a\n+++ b\n".to_string(),
            stats: None,
        }],
    }
}

/// Total visual rows a wholesale rebuild of the same blocks produces.
fn wholesale_total_rows(state: &AppState, width: usize) -> usize {
    let mut wholesale = state.transcript.clone();
    wholesale.rebuild_height_index(width);
    wholesale.total_visual_rows()
}
