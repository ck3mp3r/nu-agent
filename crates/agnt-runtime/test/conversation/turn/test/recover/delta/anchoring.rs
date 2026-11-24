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
    )
    .await?;
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
            // The run loaded one message fewer than the persist-time log holds.
            report(&full_history, dispatched_after(1)),
            2,
            PersistStage::TurnEnd {
                empty_reply: INTERRUPTED_REPLY,
            },
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id).await?;
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
    seed(&memory_state, session_id, vec![Message::user("q1")]).await?;
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
            // The run loaded the stored row and appended the steered prompt.
            report(&full_history, dispatched_after(1)),
            1,
            PersistStage::TurnEnd {
                empty_reply: INTERRUPTED_REPLY,
            },
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id).await?;
    assert_eq!(
        messages.len(),
        3,
        "the steering message and the reply are the delta; got {messages:?}"
    );
    assert_eq!(messages[1], Message::user("feedback line\n\nthe prompt"));
    assert_eq!(messages[2], Message::assistant("partial"));
    Ok(())
}

/// The delta starts at the place the run recorded for the message it sent, not
/// at the last row that happens to carry the prompt's words: an older turn's
/// identical prompt and an older steering row that ends with the prompt both
/// stay outside the delta.
#[tokio::test]
async fn persist_turn_delta_anchors_on_the_dispatched_prompt_not_its_text() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());

    // A turn that sent `q2` after an older turn asked the very same thing.
    let repeated_session = "test-recover-anchor-repeated";
    seed(
        &memory_state,
        repeated_session,
        vec![Message::user("q1"), Message::assistant("a1")],
    )
    .await?;
    let repeated_history = vec![
        Message::user("q1"),
        Message::assistant("a1"),
        Message::user("q2"),
        Message::assistant("partial"),
        Message::user("q2"),
        Message::assistant("later"),
    ];

    // A steered attempt whose older row ends with the prompt's words.
    let steered_session = "test-recover-anchor-steering-suffix";
    seed(
        &memory_state,
        steered_session,
        vec![Message::user("q1"), Message::assistant("a1")],
    )
    .await?;
    let steered_history = vec![
        Message::user("q1"),
        Message::assistant("a1"),
        Message::user("feedback line\n\nthe prompt"),
        Message::assistant("older reply"),
        Message::user("the prompt"),
        Message::assistant("partial"),
    ];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(repeated_session),
            "q2",
            report(&repeated_history, dispatched_after(2)),
            2,
            PersistStage::TurnEnd {
                empty_reply: INTERRUPTED_REPLY,
            },
            recovery(&[], None),
        )
        .await;
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(steered_session),
            "the prompt",
            report(&steered_history, dispatched_after(2)),
            2,
            PersistStage::TurnEnd {
                empty_reply: INTERRUPTED_REPLY,
            },
            recovery(&[], None),
        )
        .await;

    // -- Check
    let repeated = stored(&memory_state, repeated_session).await?;
    assert_eq!(
        repeated.len(),
        6,
        "the delta starts at this turn's own prompt row; got {repeated:?}"
    );
    assert_eq!(
        repeated[2],
        Message::user("q2"),
        "the delta's head is the prompt the turn sent; got {repeated:?}"
    );
    let steered = stored(&memory_state, steered_session).await?;
    assert_eq!(
        steered.len(),
        6,
        "the steering suffix does not bind the anchor; got {steered:?}"
    );
    assert_eq!(
        steered[2],
        Message::user("feedback line\n\nthe prompt"),
        "the delta's head is the row the anchor names, whatever it ends with; got {steered:?}"
    );
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
            // No run dispatched this turn's prompt.
            report(&full_history, None),
            0,
            PersistStage::TurnEnd {
                empty_reply: INTERRUPTED_REPLY,
            },
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id).await?;
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
