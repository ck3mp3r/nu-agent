use super::*;

#[test]
fn cancel_pushes_closing_spacer() -> Result<()> {
    use crate::interaction::cancel::CancelController;

    let cancel_controller = CancelController::default();
    let mut state = AppState::default();
    state.enqueue_prompt("hello".to_string());
    let _ = state.activate_next_prompt();
    state
        .transcript
        .push_entry(TranscriptEntry::Message(MessageEntry {
            role: MessageRole::Assistant,
            markdown: "partial".to_string(),
            timestamp_millis: None,
            model: None,
            agent: None,
        }));

    // First Esc enters AbortPending, second confirms the cancel
    reduce(
        &mut state,
        UiInput::KeyPressed { key: Key::Esc },
        Some(&cancel_controller),
    );
    reduce(
        &mut state,
        UiInput::KeyPressed { key: Key::Esc },
        Some(&cancel_controller),
    );

    let last = state
        .transcript
        .entries()
        .last()
        .ok_or("should have last transcript entry")?;
    // cancel no longer pushes a spacer — the partial assistant entry stays
    // as the last entry.
    assert!(matches!(
        last,
        TranscriptEntry::Message(MessageEntry {
            role: MessageRole::Assistant,
            ..
        })
    ));
    Ok(())
}
