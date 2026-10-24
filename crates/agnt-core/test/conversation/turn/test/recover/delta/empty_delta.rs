use super::*;

/// An empty delta persists the prompt and a placeholder reply: a turn stopped
/// before any LLM call would otherwise lose the request entirely.
#[tokio::test]
async fn persist_turn_delta_with_an_empty_delta_appends_the_prompt_and_placeholder() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-empty-delta";

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "the prompt",
            &[],
            0,
            INTERRUPTED_REPLY,
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        2,
        "an empty delta persists the prompt and a placeholder; got {messages:?}"
    );
    assert_eq!(messages[0], Message::user("the prompt"));
    assert_eq!(messages[1], Message::assistant(INTERRUPTED_REPLY));
    Ok(())
}
