use super::*;

#[test]
fn cancel_pushes_closing_spacer() -> Result<()> {
    use crate::interaction::cancel::CancelController;

    let cancel_controller = CancelController::default();
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("hello".to_string()),
        ..Default::default()
    };
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::Submit),
        Some(&cancel_controller),
    );
    let _ = state.take_next_prompt_for_execution();
    let partial = Message {
        role: MessageRole::Assistant,
        markdown: "partial".to_string(),
    };
    state.transcript.push_block(Block {
        source: partial.source(),
        lane: partial.lane(),
        fill: partial.fill(),
        status: None,
    });

    // First Esc enters AbortPending, second confirms the cancel
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::Esc),
        Some(&cancel_controller),
    );
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EscConfirm),
        Some(&cancel_controller),
    );

    let last = state
        .transcript
        .blocks()
        .last()
        .ok_or("should have last transcript block")?;
    // cancel no longer pushes a spacer — the partial assistant block stays
    // as the last block.
    assert!(matches!(
        last.source,
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            ..
        }
    ));
    Ok(())
}
