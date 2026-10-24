use super::*;

// ---------------------------------------------------------------------------
// CompletionError + hook history recovery tests
// ---------------------------------------------------------------------------

/// A hard error after prior session history persists the prompt it never
/// answered: rig saves only a successful run, so the delta is written here.
#[tokio::test]
async fn hard_error_after_prior_history_persists_the_prompt() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-hook-history";
    let mut memory_state = make_memory_state(&temp_dir);

    // Prior conversation already in the rig store, so the run starts from it.
    let prior_messages = vec![
        crate::types::Message::user("work done"),
        crate::types::Message::assistant("ok"),
    ];
    seed_rig_store(&memory_state, session_id, prior_messages.clone())?;

    // Mock model: errors immediately (simulates CompletionError / network failure).
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("http decode error")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err (it's a hard error)
    assert!(result.is_err(), "CompletionError must propagate as Err");

    // The failed turn appends its delta: the prompt it never answered. The
    // prior conversation is untouched.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        prior_messages.len() + 1,
        "a failed run appends its prompt to the prior conversation; got {persisted:?}"
    );
    assert!(
        matches!(&persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be the prior user message; got {:?}",
        persisted[0]
    );
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant(_)),
        "persisted[1] must be the prior assistant message; got {:?}",
        persisted[1]
    );
    assert_eq!(
        message_text(&persisted[2]).as_deref(),
        Some("new prompt"),
        "persisted[2] must be the failed turn's prompt; got {persisted:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Subtask 2 — a failed turn appends its delta to the log
// ---------------------------------------------------------------------------

/// A hard error after prior session history appends the delta to the rig log:
/// the prompt the turn never answered, and nothing else.
#[tokio::test]
async fn hard_error_after_prior_history_appends_the_delta() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-delta-hard-error";
    let mut memory_state = make_memory_state(&temp_dir);

    // Prior conversation already in the rig store.
    let prior_msgs = vec![
        crate::types::Message::user("prior work"),
        crate::types::Message::assistant("done"),
    ];
    seed_rig_store(&memory_state, session_id, prior_msgs)?;
    let before = rig_log(&memory_state, session_id)?;

    // Model errors immediately — simulates CompletionError / network failure.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("network timeout")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new question".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_err(), "hard error must propagate as Err");

    // The failed turn appends its delta: the log grows and the store gains the
    // prompt the turn never answered.
    let after = rig_log(&memory_state, session_id)?;
    assert_ne!(
        after, before,
        "a failed run appends its delta to the rig log"
    );
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        3,
        "the prior conversation plus the failed turn's prompt; got {persisted:?}"
    );
    assert_eq!(message_text(&persisted[2]).as_deref(), Some("new question"));

    Ok(())
}

/// Two consecutive hard errors append only their own prompt: the store grows by
/// one message per failed turn, never by a re-appended prior conversation.
#[tokio::test]
async fn hard_errors_append_only_their_own_prompt() -> Result<()> {
    let config = test_config();
    let session_id = "test-no-double";
    let temp_dir = tempfile::tempdir().unwrap();

    // Turn 1: successful turn — rig appends [user("t1"), assistant("ok")] → store has 2 msgs.
    {
        let mut memory_state = make_memory_state_on_disk(&temp_dir);
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("ok".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ]]);
        let shared_model = super::test_utils::shared_model_handle(model);
        let mut executor = make_executor(
            &config,
            &mut memory_state,
            shared_model,
            default_tool_infra(crate::bus::create_bus()),
        );
        let result = executor
            .execute(
                ExecuteInput {
                    prompt: "t1".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
        assert!(result.is_ok(), "turn 1 must succeed");
    }

    // Turn 2: hard error. pre_turn_count=2. last_known_history = [prior_1, prior_2, user("t2")].
    // delta = skip(2) = [user("t2")] → delta path fires → store has 3.
    {
        let mut memory_state = make_memory_state_on_disk(&temp_dir);
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error("timeout")]]);
        let shared_model = super::test_utils::shared_model_handle(model);
        let mut executor = make_executor(
            &config,
            &mut memory_state,
            shared_model,
            default_tool_infra(crate::bus::create_bus()),
        );
        let _ = executor
            .execute(
                ExecuteInput {
                    prompt: "t2".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
    }

    // Turn 3: hard error again. pre_turn_count=3. last_known_history = [prior_1, prior_2, user("t2"), user("t3")].
    // delta = skip(3) = [user("t3")] → delta path fires → store has 4.
    {
        let mut memory_state = make_memory_state_on_disk(&temp_dir);
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error("timeout")]]);
        let shared_model = super::test_utils::shared_model_handle(model);
        let mut executor = make_executor(
            &config,
            &mut memory_state,
            shared_model,
            default_tool_infra(crate::bus::create_bus()),
        );
        let _ = executor
            .execute(
                ExecuteInput {
                    prompt: "t3".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
    }

    // Final state: turn 1 saved its exchange, and each failed turn appended
    // only its own prompt.
    let final_memory_state = make_memory_state_on_disk(&temp_dir);
    let final_messages = load_persisted_messages(&final_memory_state, session_id).await?;

    assert_eq!(
        final_messages.len(),
        4,
        "turn 1's exchange plus one prompt per failed turn; got {final_messages:?}"
    );
    assert_eq!(message_text(&final_messages[2]).as_deref(), Some("t2"));
    assert_eq!(message_text(&final_messages[3]).as_deref(), Some("t3"));

    Ok(())
}
