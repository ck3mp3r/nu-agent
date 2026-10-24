use super::*;

/// A store whose length moved since the turn started is left alone: slicing it
/// would mis-attribute another writer's messages. The skip loses the turn's
/// work, so it publishes a user-visible warning.
#[tokio::test]
async fn persist_turn_delta_skips_when_the_store_moved_mid_turn() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-moved";
    let prior = vec![Message::user("q1"), Message::assistant("a1")];
    seed(&memory_state, session_id, prior.clone())?;
    let full_history = vec![Message::user("q1"), Message::assistant("a1")];
    let bus = crate::bus::create_bus();
    let mut ui_events = bus.ui_event().subscribe();

    // -- Exec: the turn claims it started from one message, the store holds two
    executor_with_bus(&config, &mut memory_state, bus)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            1,
            INTERRUPTED_REPLY,
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages, prior,
        "a moved store must be left untouched; got {messages:?}"
    );
    assert_eq!(
        warnings(&mut ui_events),
        vec![MOVED_MID_TURN_WARNING.to_string()],
        "the skip must warn the user that the turn's work was not saved"
    );
    Ok(())
}

/// A session-less turn has no rig log to write to.
#[tokio::test]
async fn persist_turn_delta_without_a_session_writes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-no-session";

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            None,
            "the prompt",
            &[Message::user("the prompt")],
            0,
            INTERRUPTED_REPLY,
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert!(
        messages.is_empty(),
        "a session-less turn writes nothing; got {messages:?}"
    );
    Ok(())
}
