use super::*;

/// Enqueuing a prompt queues it without growing the transcript; activating the
/// next prompt appends exactly one user block.
#[test]
fn enqueue_and_activate_prompt_transcript_bookkeeping() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let before = state.transcript.len();

    // -- Exec & Check: enqueue does not add a transcript entry.
    state.enqueue_prompt("second".to_string());
    assert_eq!(state.transcript.len(), before);

    // -- Exec & Check: an external prompt queues without growing the transcript.
    let mut external = AppState::default();
    let before = external.transcript.len();
    external.reduce_ui_state_event(UiStateEvent::EnqueueExternalPrompt {
        text: "mailbox message".to_string(),
    });
    assert_eq!(external.phase, UiPhase::Busy);
    assert_eq!(external.prompt_items().len(), 1);
    assert_eq!(external.prompt_items()[0].status, PromptStatus::Queued);
    assert_eq!(external.prompt_items()[0].prompt_text, "mailbox message");
    assert_eq!(pending_prompt_ids(&external), vec![1]);
    assert_eq!(active_prompt_id(&external), None);
    assert_eq!(external.transcript.len(), before);

    // -- Exec & Check: activating the next prompt appends one user block.
    let mut activating = AppState::default();
    activating.enqueue_prompt("first".to_string());
    let _ = activating.take_next_prompt_for_execution();
    activating.enqueue_prompt("second".to_string());
    activating.complete_active_prompt();
    let before = activating.transcript.len();
    activating.activate_next_prompt();
    assert_eq!(activating.transcript.len(), before + 1);
    let last = activating
        .transcript
        .entries()
        .last()
        .ok_or("should have last transcript entry")?;
    assert!(matches!(last, TranscriptEntry::Message(_)));

    // -- Setup & Fixtures: token usage recording.
    let mut state = AppState::default();

    // -- Exec & Check: latest values and accumulated session total.
    state.status.tokens.record_token_usage(7, 5, Some(12));
    assert_eq!(state.status.tokens.latest_input_tokens, Some(7));
    assert_eq!(state.status.tokens.latest_output_tokens, Some(5));
    assert_eq!(state.status.tokens.latest_total_tokens, Some(12));
    assert_eq!(state.status.tokens.session_total_tokens, 12);
    state.status.tokens.record_token_usage(2, 3, Some(5));
    assert_eq!(state.status.tokens.latest_input_tokens, Some(2));
    assert_eq!(state.status.tokens.latest_output_tokens, Some(3));
    assert_eq!(state.status.tokens.latest_total_tokens, Some(5));
    assert_eq!(state.status.tokens.session_total_tokens, 17);

    // -- Setup & Fixtures: a state with recorded tokens.
    let mut cleared = AppState {
        status: StatusState {
            tokens: TokenUsage {
                latest_input_tokens: Some(100),
                latest_output_tokens: Some(200),
                latest_total_tokens: Some(300),
                ..Default::default()
            },
            ..Default::default()
        },
        ..AppState::default()
    };

    // -- Exec
    cleared.clear_transcript();

    // -- Check
    assert!(cleared.status.tokens.latest_input_tokens.is_none());
    assert!(cleared.status.tokens.latest_output_tokens.is_none());
    assert!(cleared.status.tokens.latest_total_tokens.is_none());
    Ok(())
}
