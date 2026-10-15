use super::*;

/// Pushing blocks grows the store by one content block each (no auto-inserted
/// spacers), and a user prompt queued during an external prompt is delivered
/// exactly once after the external prompt completes.
#[test]
fn push_block_and_external_prompt_delivery() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let msg = Message {
        role: MessageRole::User,
        markdown: "hi".to_string(),
    };

    // -- Exec
    state.transcript.push_block(Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: msg.fill(),
        status: None,
    });
    let msg2 = Message {
        role: MessageRole::Assistant,
        markdown: "reply".to_string(),
    };
    state.transcript.push_block(Block {
        source: msg2.source(),
        lane: msg2.lane(),
        fill: msg2.fill(),
        status: None,
    });

    // -- Check: two content blocks, no spacers.
    assert_eq!(state.transcript.len(), 2);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Markdown { .. }
    ));
    assert!(matches!(
        state.transcript.blocks()[1].source,
        BlockSource::Markdown { .. }
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
