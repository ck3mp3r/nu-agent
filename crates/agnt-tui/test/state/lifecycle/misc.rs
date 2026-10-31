use super::*;

/// Pushing entries grows the store by one content entry each (no auto-inserted
/// spacers), and a user prompt queued during an external prompt is delivered
/// exactly once after the external prompt completes.
#[test]
fn push_entry_and_external_prompt_delivery() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    state
        .transcript
        .push_entry(TranscriptEntry::Message(MessageEntry {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        }));
    state
        .transcript
        .push_entry(TranscriptEntry::Message(MessageEntry {
            role: MessageRole::Assistant,
            markdown: "reply".to_string(),
        }));

    // -- Check: two content entries, no spacers.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.entries()[0],
        TranscriptEntry::Message(_)
    ));
    assert!(matches!(
        state.transcript.entries()[1],
        TranscriptEntry::Message(_)
    ));

    // -- Setup & Fixtures: an external prompt is active.
    let mut queued = AppState::default();
    queued.enqueue_prompt("external task".to_string());
    let _ = queued.take_next_prompt_for_execution();
    assert_eq!(active_prompt_id(&queued), Some(1));

    // -- Exec: a user prompt arrives while the external prompt runs.
    queued.enqueue_prompt("user task".to_string());

    // -- Check: the user prompt waits for the external prompt to complete.
    assert_eq!(queued.take_next_prompt_for_execution(), None);
    queued.complete_active_prompt();
    assert_eq!(
        queued.take_next_prompt_for_execution().as_deref(),
        Some("user task")
    );
    assert_eq!(queued.take_next_prompt_for_execution(), None);
    Ok(())
}

/// Disarming an armed confirmation clears the confirm message and reports the
/// visible change.
#[test]
fn test_lifecycle_disarm_quit_confirmation_armed_reports_change() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.arm_quit_confirmation();
    assert_eq!(
        state.status.message.status_line(),
        CTRL_C_QUIT_CONFIRM_STATUS
    );

    // -- Exec
    let changed = state.disarm_quit_confirmation();

    // -- Check: the confirm message was cleared.
    assert!(changed);
    assert!(state.quit_confirm_since.is_none());
    assert!(state.status.message.status_line().is_empty());
    Ok(())
}

/// Disarming while not armed reports no change and leaves the status message
/// untouched.
#[test]
fn test_lifecycle_disarm_quit_confirmation_not_armed_reports_no_change() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.status.message.set_message("keep me");

    // -- Exec
    let changed = state.disarm_quit_confirmation();

    // -- Check
    assert!(!changed);
    assert_eq!(state.status.message.status_line(), "keep me");
    Ok(())
}

/// An expired confirmation is not armed, so disarming reports no change and
/// leaves the message to its own TTL.
#[test]
fn test_lifecycle_disarm_quit_confirmation_expired_reports_no_change() -> Result<()> {
    // -- Setup & Fixtures: a zero-length confirmation window.
    let mut state = AppState {
        quit_confirm_window: std::time::Duration::from_millis(0),
        ..Default::default()
    };
    state.arm_quit_confirmation();

    // -- Exec
    let changed = state.disarm_quit_confirmation();

    // -- Check
    assert!(!changed);
    assert_eq!(
        state.status.message.status_line(),
        CTRL_C_QUIT_CONFIRM_STATUS
    );
    Ok(())
}
