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
        &ToolCallId::of("read", "{}"),
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
        &ToolCallId::of("edit", EDIT_ARGUMENTS),
        &preview,
    );

    // -- Check: the preview lands on the entry and pushes nothing.
    assert_eq!(preview_state.transcript.len(), 1);
    let entry = tool_entry_at(&preview_state, 0)?;
    assert_eq!(entry.preview.as_ref(), Some(&preview));

    // -- Check: the in-place mutation marks the index stale; the rebuild
    // accounts for the preview rows.
    assert!(
        !preview_state.transcript.height_index_valid_for(width),
        "an in-place preview mutation must invalidate the height index"
    );
    preview_state.transcript.rebuild_height_index(width);
    assert!(
        preview_state.transcript.total_visual_rows() > rows_before,
        "the preview content must add visual rows after the rebuild"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Preview-path height index (task 69706655)
// ---------------------------------------------------------------------------

/// `set_tool_preview` mutates the tool entry in place and marks the height
/// index stale. A rebuild must therefore agree with a wholesale rebuild, both
/// when the tool entry is last and when another entry follows it.
#[test]
fn set_tool_preview_invalidates_and_rebuilds_to_a_wholesale_index() -> Result<()> {
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
        &ToolCallId::of("edit", EDIT_ARGUMENTS),
        &edit_preview(),
    );

    // -- Check: one entry, stale index, rebuild matches a wholesale rebuild.
    assert_eq!(state.transcript.len(), 1);
    assert!(!state.transcript.height_index_valid_for(width));
    state.transcript.rebuild_height_index(width);
    assert_eq!(
        state.transcript.total_visual_rows(),
        wholesale_total_rows(&state, width),
        "the rebuilt index must match a wholesale rebuild after a tail preview"
    );

    // -- Setup & Fixtures: a user entry after the tool entry.
    let mut mid = AppState::default();
    mid.tool
        .start_tool_call(&mut mid.transcript, &tool_call("edit", EDIT_ARGUMENTS));
    mid.transcript
        .push_entry(TranscriptEntry::Message(MessageEntry {
            role: MessageRole::User,
            markdown: "later turn".to_string(),
        }));
    mid.transcript.rebuild_height_index(width);
    assert!(
        mid.transcript.height_index_valid_for(width),
        "precondition: index must be valid before the preview is set"
    );

    // -- Exec: the preview lands on the first entry, before the user entry.
    mid.tool.set_tool_preview(
        &mut mid.transcript,
        &ToolCallId::of("edit", EDIT_ARGUMENTS),
        &edit_preview(),
    );

    // -- Check: the mutation keeps the entry count, invalidates the index, and
    // the rebuild matches a wholesale rebuild.
    assert_eq!(mid.transcript.len(), 2);
    assert!(!mid.transcript.height_index_valid_for(width));
    mid.transcript.rebuild_height_index(width);
    assert_eq!(
        mid.transcript.total_visual_rows(),
        wholesale_total_rows(&mid, width),
        "the rebuilt index must match a wholesale rebuild after a mid-list preview"
    );
    Ok(())
}

// -- Test Support

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
