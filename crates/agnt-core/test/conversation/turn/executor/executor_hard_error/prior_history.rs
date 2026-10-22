use super::*;

// ---------------------------------------------------------------------------
// CompletionError + hook history recovery tests
// ---------------------------------------------------------------------------

/// A hard error after prior session history saves nothing: rig saves only a
/// successful run, so the rig store keeps exactly the prior conversation.
#[tokio::test]
async fn hard_error_after_prior_history_saves_nothing() -> Result<()> {
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

    // rig saved nothing: the store still holds exactly the prior conversation.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        prior_messages.len(),
        "a failed run must leave the rig store unchanged; got {persisted:?}"
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

    Ok(())
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Subtask 2 — a failed turn must not touch the log
// ---------------------------------------------------------------------------

/// A hard error after prior session history leaves the rig log byte-identical:
/// no delta re-append, no placeholder pair.
#[tokio::test]
async fn hard_error_after_prior_history_leaves_the_log_unchanged() -> Result<()> {
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

    // rig saved nothing: the log is byte-identical.
    let after = rig_log(&memory_state, session_id)?;
    assert_eq!(
        after, before,
        "a failed run must leave the rig log byte-identical"
    );

    Ok(())
}

/// Two consecutive hard errors must not grow the rig store: only the successful
/// turn is saved.
#[tokio::test]
async fn hard_errors_do_not_grow_the_store() -> Result<()> {
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

    // Final state: only turn 1 was saved, so the rig store holds 2 messages.
    // The two failed turns saved nothing.
    let final_memory_state = make_memory_state_on_disk(&temp_dir);
    let final_messages = load_persisted_messages(&final_memory_state, session_id).await?;

    assert_eq!(
        final_messages.len(),
        2,
        "two hard errors after one success must leave the rig store at the 2 \
         messages turn 1 saved; got {final_messages:?}"
    );

    Ok(())
}
