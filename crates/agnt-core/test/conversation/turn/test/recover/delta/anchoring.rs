use super::*;

/// The delta is anchored on the prompt, not on the pre-turn count: a count one
/// ahead of the run's own load would start the slice at the committed reply and
/// lose the prompt.
#[tokio::test]
async fn persist_turn_delta_anchors_the_delta_on_the_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-anchor";
    seed(
        &memory_state,
        session_id,
        vec![Message::user("q1"), Message::assistant("a1")],
    )?;
    // The run loaded one message fewer than the persist-time log holds, so its
    // prompt sits at index 1 while the count slice would start at index 2.
    let full_history = vec![
        Message::user("q1"),
        Message::user("q2"),
        Message::assistant("partial"),
        assistant_tool_calls(&["tc1"])?,
    ];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            &full_history,
            2,
            INTERRUPTED_REPLY,
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        6,
        "the prior conversation plus the anchored delta; got {messages:?}"
    );
    assert_eq!(
        messages[2],
        Message::user("q2"),
        "the prompt is persisted; got {messages:?}"
    );
    assert_eq!(
        messages[3],
        Message::assistant("partial"),
        "the committed reply follows the prompt; got {messages:?}"
    );
    Ok(())
}

/// A steering retry runs with `"{feedback}\n\n{prompt}"`, so the anchor matches
/// a user message that ends with the prompt.
#[tokio::test]
async fn persist_turn_delta_matches_the_steering_augmented_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-steering";
    seed(&memory_state, session_id, vec![Message::user("q1")])?;
    let full_history = vec![
        Message::user("q1"),
        Message::user("feedback line\n\nthe prompt"),
        Message::assistant("partial"),
    ];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "the prompt",
            &full_history,
            1,
            INTERRUPTED_REPLY,
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        3,
        "the steering message and the reply are the delta; got {messages:?}"
    );
    assert_eq!(messages[1], Message::user("feedback line\n\nthe prompt"));
    assert_eq!(messages[2], Message::assistant("partial"));
    Ok(())
}

/// A history that does not hold the prompt and carries no records falls back to
/// the prompt and the placeholder reply.
#[tokio::test]
async fn persist_turn_delta_falls_back_when_the_prompt_is_absent() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-no-prompt";
    let full_history = vec![Message::user("something else")];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "the prompt",
            &full_history,
            0,
            INTERRUPTED_REPLY,
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages,
        vec![
            Message::user("the prompt"),
            Message::assistant(INTERRUPTED_REPLY)
        ],
        "the fallback pair is persisted; got {messages:?}"
    );
    Ok(())
}

/// A history that does not hold the prompt but carries records persists the
/// prompt and the reconstructed exchange.
#[tokio::test]
async fn persist_turn_delta_reconstructs_the_exchange_when_the_prompt_is_absent() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-no-prompt-records";
    let full_history = vec![Message::user("something else")];
    let records = vec![record("tc1", Some("file contents"))?, record("tc2", None)?];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "the prompt",
            &full_history,
            0,
            INTERRUPTED_REPLY,
            recovery(&records, None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id)?;
    assert_eq!(
        messages.len(),
        3,
        "the prompt plus the reconstructed exchange; got {messages:?}"
    );
    assert_eq!(
        messages[0],
        Message::user("the prompt"),
        "the prompt is reconstructed; got {messages:?}"
    );
    let Message::Assistant(assistant) = &messages[1] else {
        return Err("the reconstructed calls must be an assistant message".into());
    };
    let calls: Vec<&CallId> = assistant.tool_calls().map(|call| &call.id).collect();
    assert_eq!(
        calls,
        vec![&CallId::from_wire("tc1"), &CallId::from_wire("tc2")]
    );
    assert_eq!(
        result_texts(&messages[2]),
        vec!["file contents".to_string(), "[interrupted]".to_string()],
        "a settled call keeps its result, an unsettled one is interrupted; got {messages:?}"
    );
    Ok(())
}
