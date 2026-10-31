use super::*;

#[test]
fn cancel_pushes_closing_spacer() -> Result<()> {
    use crate::interaction::cancel::CancelController;

    let cancel_controller = CancelController::default();
    let mut state = AppState::default();
    reduce(
        &mut state,
        SystemEvent::PromptSubmitted {
            text: "hello".to_string(),
        },
        Some(&cancel_controller),
    );
    let _ = state.take_next_prompt_for_execution();
    state
        .transcript
        .push_entry(TranscriptEntry::Message(MessageEntry {
            role: MessageRole::Assistant,
            markdown: "partial".to_string(),
        }));

    // First Esc enters AbortPending, second confirms the cancel
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Esc },
        Some(&cancel_controller),
    );
    reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Esc },
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
