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

    // -- Exec & Check: a notice stores content only.
    let mut mixed = AppState::default();
    mixed.transcript.push_entry(TranscriptEntry::Notice(
        agnt_core::transcript::entry::NoticeEntry {
            kind: NoticeKind::Compaction,
            text: "Compaction".to_string(),
        },
    ));
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
