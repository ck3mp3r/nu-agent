use super::*;

/// The streamed text the delta's committed assistant text does not already hold
/// is appended as one assistant message.
#[tokio::test]
async fn persist_turn_delta_appends_the_streamed_prose_remainder() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-prose";
    let full_history = vec![Message::user("q2")];
    let records = vec![record("tc1", Some("a"))?, record("tc2", Some("b"))?];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            0,
            INTERRUPTED_REPLY,
            recovery(&records, Some("partial answer")),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        4,
        "the prompt, the exchange and the streamed prose; got {messages:?}"
    );
    assert_eq!(
        result_texts(&messages[2]),
        vec!["a".to_string(), "b".to_string()],
        "the settled results are persisted; got {messages:?}"
    );
    assert_eq!(
        messages[3],
        Message::assistant("partial answer"),
        "the streamed prose is persisted after the exchange; got {messages:?}"
    );
    Ok(())
}

/// Streamed text that does not extend the committed assistant text is dropped:
/// persisting it would duplicate or contradict what the log already holds.
#[tokio::test]
async fn persist_turn_delta_drops_prose_that_does_not_extend_the_committed_text() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-prose-mismatch";
    let full_history = vec![Message::user("q2"), Message::assistant("committed")];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            0,
            INTERRUPTED_REPLY,
            recovery(&[], Some("a different answer")),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        2,
        "the mismatched prose is not persisted; got {messages:?}"
    );
    Ok(())
}
