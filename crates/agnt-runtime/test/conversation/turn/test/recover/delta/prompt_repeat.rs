use super::*;

use crate::types::Text;

/// A retried attempt reports the prompt again on top of a log this turn already
/// persisted it to. Mid-flight the prompt belongs to the turn's end, so the
/// delta carries nothing and the log stays as the seed left it.
#[tokio::test]
async fn test_persist_turn_delta_mid_flight_plain_persist_drops_the_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-midflight-plain";
    // The log after the turn's first attempt: it already carries the prompt.
    let prior = vec![Message::user("q1"), Message::user("q2")];
    seed(&memory_state, session_id, prior.clone()).await?;
    // The retry loaded that log and rig appended the prompt once more.
    let full_history = vec![
        Message::user("q1"),
        Message::user("q2"),
        Message::user("q2"),
    ];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            // The retry loaded the prior log and appended the prompt once more.
            report(&full_history, dispatched_after(prior.len())),
            prior.len(),
            PersistStage::MidFlight { keep_head: false },
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id).await?;
    assert_eq!(
        messages, prior,
        "a mid-flight persist writes no prompt row; got {messages:?}"
    );
    // No facts assertion here: the emptied delta skips the save and the settle
    // together, and an in-memory fact store opens a fresh database per call, so
    // a read-back could not tell the difference either way.
    Ok(())
}

/// Mid-flight carries content only, whatever the log holds: an older turn's
/// identical prompt row does not make this delta persist anything.
#[tokio::test]
async fn test_persist_turn_delta_mid_flight_writes_no_prompt_over_an_older_copy() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-midflight-older";
    // An older turn completed with the very text this attempt sends.
    let prior = vec![Message::user("q2"), Message::assistant("a")];
    seed(&memory_state, session_id, prior.clone()).await?;
    let mut full_history = prior.clone();
    full_history.push(Message::user("q2"));

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            // The retry loaded the prior log and appended the prompt once more.
            report(&full_history, dispatched_after(prior.len())),
            prior.len(),
            PersistStage::MidFlight { keep_head: false },
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id).await?;
    assert_eq!(
        messages, prior,
        "the skipped delta leaves the log exactly as the seed left it; got {messages:?}"
    );
    Ok(())
}

/// A mid-flight delta whose history does not carry the prompt has nothing to
/// persist: the turn's request lands at its end, not here.
#[tokio::test]
async fn test_persist_turn_delta_mid_flight_anchor_absent_writes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-anchor-absent";
    let prior = vec![Message::user("q1"), Message::user("q2")];
    seed(&memory_state, session_id, prior.clone()).await?;

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            // The turn reported no history at all.
            report(&[], None),
            prior.len(),
            PersistStage::MidFlight { keep_head: false },
            recovery(&[], None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id).await?;
    assert_eq!(
        messages, prior,
        "a mid-flight delta with no anchor writes nothing; got {messages:?}"
    );
    Ok(())
}

/// A terminal event whose history carries no prompt still records the request:
/// the anchored delta is empty, so the pair fills it once.
#[tokio::test]
async fn test_persist_turn_delta_turn_end_anchor_absent_persists_the_pair() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-turn-end-anchor-absent";
    let full_history = vec![Message::user("something else")];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "the prompt",
            // The turn reported a history that does not hold its prompt.
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
        "a terminal event with no anchor persists the request once; got {messages:?}"
    );
    Ok(())
}

/// Mid-flight keeps steering content and the results a merged row carries, and
/// drops only the prompt text.
#[tokio::test]
async fn test_persist_turn_delta_mid_flight_keeps_steering_and_merged_results() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let merged_session = "test-recover-merged-row";
    seed(
        &memory_state,
        merged_session,
        vec![Message::user("q1"), Message::assistant("a")],
    )
    .await?;
    // rig's repair merged the prompt with a committed result row.
    let merged = Message::User {
        content: vec![
            UserContent::Text(Text::new("q2")),
            UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc9"),
                name: ToolName::new("read_file")?,
                content: vec![ToolResultContent::text("stored output")],
                is_error: false,
            }),
        ],
    };
    let merged_history = vec![Message::user("q1"), merged.clone()];

    let steered_session = "test-recover-steered-row";
    seed(
        &memory_state,
        steered_session,
        vec![Message::user("q1"), Message::user("the prompt")],
    )
    .await?;
    let steered = Message::user("feedback line\n\nthe prompt");
    let steered_history = vec![
        Message::user("q1"),
        Message::user("the prompt"),
        steered.clone(),
        Message::assistant("partial"),
    ];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(merged_session),
            "q2",
            // The repair fused the prompt into the row it follows.
            report(&merged_history, dispatched_after(1)),
            2,
            PersistStage::MidFlight { keep_head: false },
            recovery(&[], None),
        )
        .await;
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(steered_session),
            "the prompt",
            // The steered attempt's own prompt row.
            report(&steered_history, dispatched_after(2)),
            2,
            PersistStage::MidFlight { keep_head: true },
            recovery(&[], None),
        )
        .await;

    // -- Check
    let merged_messages = stored(&memory_state, merged_session).await?;
    assert_eq!(
        merged_messages.len(),
        3,
        "the merged row's results still persist; got {merged_messages:?}"
    );
    let Message::User { content } = &merged_messages[2] else {
        return Err("the merged row must stay a user message".into());
    };
    assert!(
        content
            .iter()
            .all(|item| matches!(item, UserContent::ToolResult(_))),
        "the merged row loses its prompt text and keeps its result; got {merged_messages:?}"
    );
    assert_eq!(
        result_texts(&merged_messages[2]),
        vec!["stored output".to_string()],
        "the committed result is not lost; got {merged_messages:?}"
    );

    let steered_messages = stored(&memory_state, steered_session).await?;
    assert_eq!(
        steered_messages.len(),
        4,
        "the steering row is content the turn keeps; got {steered_messages:?}"
    );
    assert_eq!(
        steered_messages[2], steered,
        "the steering message keeps its feedback text; got {steered_messages:?}"
    );
    assert_eq!(steered_messages[3], Message::assistant("partial"));
    Ok(())
}

/// A terminal event with nothing else reported persists the request: a turn
/// stopped before any LLM call must not lose it.
#[tokio::test]
async fn test_persist_turn_delta_turn_end_persists_the_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-turn-end";

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "the prompt",
            // The turn reported nothing: no run dispatched a prompt.
            report(&[], None),
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
        "the first attempt persists the prompt and the placeholder; got {messages:?}"
    );
    Ok(())
}

/// A terminal write keeps its own prompt row whatever older rows hold: a
/// resend lands its request even after an identical completed turn.
#[tokio::test]
async fn test_persist_turn_delta_resend_after_older_turn_persists() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-resend-after-turn";
    // An older turn completed with the very text this turn sends.
    let prior = vec![Message::user("fix it"), Message::assistant("done")];
    seed(&memory_state, session_id, prior.clone()).await?;
    let mut full_history = prior.clone();
    full_history.push(Message::user("fix it"));

    // -- Exec: this turn starts after the older turn's two rows.
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "fix it",
            report(&full_history, dispatched_after(2)),
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
        3,
        "the older copy did not consume this turn's prompt; got {messages:?}"
    );
    assert_eq!(
        messages[2],
        Message::user("fix it"),
        "the resend's own prompt row is persisted; got {messages:?}"
    );
    Ok(())
}

/// An older steering-form row does not affect a terminal write either.
#[tokio::test]
async fn test_persist_turn_delta_resend_after_older_steering_persists() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-resend-after-steering";
    let prior = vec![
        Message::user("fix it"),
        Message::assistant("a"),
        Message::user("feedback\n\nfix it"),
        Message::assistant("b"),
    ];
    seed(&memory_state, session_id, prior.clone()).await?;
    let mut full_history = prior.clone();
    full_history.push(Message::user("fix it"));

    // -- Exec: this turn starts after the older turn's four rows.
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "fix it",
            report(&full_history, dispatched_after(4)),
            4,
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
        5,
        "the older steering row did not trip the repeat rule; got {messages:?}"
    );
    assert_eq!(
        messages[4],
        Message::user("fix it"),
        "the resend's own prompt row is persisted; got {messages:?}"
    );
    Ok(())
}

/// A mid-flight persist drops the prompt but lands the attempt's tool work: the
/// call row and the result row land exactly once.
#[tokio::test]
async fn test_persist_turn_delta_mid_flight_tool_work_persists_the_exchange_once() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config::default();
    let mut memory_state = MemoryState::new(RigStoreConfig::in_memory());
    let session_id = "test-recover-retry-tool-work";
    // Attempt 0 persisted the prompt inside this turn's slice.
    let prior = vec![Message::user("q1"), Message::user("q2")];
    seed(&memory_state, session_id, prior.clone()).await?;
    let full_history = vec![
        Message::user("q1"),
        Message::user("q2"),
        Message::user("q2"),
        assistant_tool_calls(&["tc1"])?,
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc1"),
                name: ToolName::new("read_file")?,
                content: vec![ToolResultContent::text("ok")],
                is_error: false,
            })],
        },
    ];
    let records = vec![record("tc1", Some("ok"))?];

    // -- Exec
    executor(&config, &mut memory_state)
        .persist_turn_delta(
            Some(session_id),
            "q2",
            // The retry loaded the prior log and appended the prompt once more.
            report(&full_history, dispatched_after(2)),
            2,
            PersistStage::MidFlight { keep_head: false },
            recovery(&records, None),
        )
        .await;

    // -- Check
    let messages = stored(&memory_state, session_id).await?;
    assert_eq!(
        messages.len(),
        prior.len() + 2,
        "the call row and its result row land once; got {messages:?}"
    );
    let prompts = messages
        .iter()
        .filter(|message| **message == Message::user("q2"))
        .count();
    assert_eq!(
        prompts, 1,
        "the retried prompt is not appended a second time; got {messages:?}"
    );
    let Message::Assistant(assistant) = &messages[prior.len()] else {
        return Err("the exchange must start with the assistant call row".into());
    };
    let calls: Vec<&CallId> = assistant.tool_calls().map(|call| &call.id).collect();
    assert_eq!(
        calls,
        vec![&CallId::from_wire("tc1")],
        "the call is persisted exactly once; got {messages:?}"
    );
    assert_eq!(
        result_texts(&messages[prior.len() + 1]),
        vec!["ok".to_string()],
        "the settled result is persisted exactly once; got {messages:?}"
    );
    // No facts assertion here: an in-memory fact store opens a fresh database
    // per call, so the settle it recorded could not be read back.
    Ok(())
}

// region:    --- Modules

#[path = "prompt_repeat/fail_then_success.rs"]
mod fail_then_success;

// endregion: --- Modules
