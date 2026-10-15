use super::*;

// ---------------------------------------------------------------------------
// CompletionError + hook history recovery tests
// ---------------------------------------------------------------------------

/// Hard error after prior session history: the user prompt is persisted via the delta
/// path (not a placeholder pair), because `last_known_history` now includes the prompt.
///
/// **Why the delta is [user_prompt] after the fix:** `on_completion_call(prompt, history)`
/// now stores `history + [prompt]`. With prior history = [prior_1, prior_2] and a new
/// user prompt, `last_known_history` = [prior_1, prior_2, user_prompt].
/// `skip(pre_turn_message_count=2)` = [user_prompt] → delta path fires → 1 new message.
///
/// Before the fix: `last_known_history` = [prior_1, prior_2] (no prompt included).
/// `skip(2)` = empty delta → placeholder pair fired → 4 messages (2 prior + 2 placeholder).
///
/// After the fix: delta = [user_prompt] → delta path fires → 3 messages total
/// (2 prior + 1 user_prompt). The placeholder path no longer fires.
///
/// Key regression assertion: store must be exactly 3 (2 prior + 1 user prompt),
/// NOT 4 (old placeholder pair) and NOT 5 (the doubled result from the pre-delta-fix bug).
#[tokio::test]
async fn hard_error_after_prior_history_persists_user_message() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-hook-history";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate the session store with a completed exchange so that rig
    // loads it into the agent's context and on_completion_call fires with
    // non-empty prior history.
    let prior_messages = vec![
        crate::types::Message::user("work done"),
        crate::types::Message::assistant("ok"),
    ];
    // Pre-populate the store by creating a separate FsSessionStore pointing to the same path
    // (the memory_state's internal store shares the same backing directory)
    let _prior_entries: Vec<StoreEntry> = prior_messages
        .iter()
        .cloned()
        .map(StoreEntry::Message)
        .collect();
    memory_state.inner_memory().load_all(session_id).await.ok();
    // Use ConversationMemory append to pre-populate
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(
                &rig::id::ConversationId::from(session_id),
                prior_messages.clone(),
            )
            .await
            .map_err(|e| format!("append prior messages: {e:?}"))?;
    }

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

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // After the fix: last_known_history = [prior_1, prior_2, user_prompt].
    // delta = skip(2) = [user_prompt] → delta path fires → 3 messages total.
    // NOT 4 (old placeholder pair), NOT 5 (doubled history from pre-delta bug).
    assert_eq!(
        persisted.len(),
        3,
        "hard error after prior history must persist exactly 3 messages \
         (2 prior + 1 user prompt via delta path); got {:?}",
        persisted
    );

    // [0] must be the prior user message
    assert!(
        matches!(&persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be User (prior); got {:?}",
        persisted[0]
    );
    // [1] must be the prior assistant message
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant(_)),
        "persisted[1] must be Assistant (prior); got {:?}",
        persisted[1]
    );
    // [2] must be the user prompt ("new prompt")
    assert!(
        matches!(&persisted[2], crate::types::Message::User { .. }),
        "persisted[2] must be User (new prompt); got {:?}",
        persisted[2]
    );
    // Verify [2] contains the new prompt text
    let new_prompt_text = match &persisted[2] {
        crate::types::Message::User { content } => content
            .iter()
            .find_map(|c| {
                if let crate::types::UserContent::Text(t) = c {
                    Some(t.text.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        other => panic!("persisted[2] must be User; got {:?}", other),
    };
    assert_eq!(
        new_prompt_text, "new prompt",
        "persisted[2] must contain the new user prompt text"
    );

    // Prior message content check
    let has_prior_user = persisted.iter().any(|msg| {
        if let crate::types::Message::User { content } = msg {
            content.iter().any(|item| {
                if let crate::types::UserContent::Text(t) = item {
                    t.text == "work done"
                } else {
                    false
                }
            })
        } else {
            false
        }
    });
    assert!(
        has_prior_user,
        "prior user message must still be in store; got messages: {:?}",
        persisted
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Subtask 2 — delta-only persistence regression tests
// ---------------------------------------------------------------------------

/// Regression test: hard error after prior session history must not re-append the
/// full hook history snapshot — only a delta (the user prompt from this turn).
///
/// Before the fix: `last_known_history` (full history) was appended → store grew
/// from 2 to 5 messages (2 prior + 3 full history re-appended).
/// After the fix: `last_known_history` = [prior_1, prior_2, user_prompt].
/// delta = skip(pre_turn_count=2) = [user_prompt] → delta path fires → 3 messages total.
///
/// The bound is now exactly 3 (not < 5). The delta path fires with just the user
/// prompt — no placeholder is synthesised because the delta is non-empty.
#[tokio::test]
async fn hard_error_after_prior_history_persists_only_delta() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-delta-hard-error";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate: simulate a prior successful turn using ConversationMemory append
    let prior_msgs = vec![
        crate::types::Message::user("prior work"),
        crate::types::Message::assistant("done"),
    ];
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(&rig::id::ConversationId::from(session_id), prior_msgs)
            .await
            .map_err(|e| format!("append prior messages: {e:?}"))?;
    }

    // Model errors immediately — simulates CompletionError / network failure.
    // on_completion_call fires with history = [user("prior work"), assistant("done")]
    // and prompt = user("new question").
    // After fix: last_known_history = [prior_1, prior_2, user_prompt].
    // delta = skip(2) = [user_prompt] → delta path fires → 3 total.
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

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // After the fix: exactly 3 messages (2 prior + 1 user prompt delta).
    // The delta path fires because last_known_history includes the user prompt.
    // NOT 5 (pre-delta-fix duplication bug), NOT 4 (old placeholder pair).
    assert_eq!(
        persisted.len(),
        3,
        "hard error after prior history must persist exactly 3 messages (2 prior + 1 user prompt delta); got {} messages",
        persisted.len()
    );
    assert!(
        persisted.len() >= 2,
        "prior messages must be preserved; got {}",
        persisted.len()
    );

    Ok(())
}

/// Regression test: two consecutive hard errors must not double history each time.
///
/// Before the fix: turn 2 error appended full history (3 msgs), turn 3 appended full
/// history again (4 msgs) → store grew to 2 + 3 + 4 = 9 messages.
/// After the fix: `on_completion_call` stores `history + [prompt]`, so for each error
/// turn the delta = [user_prompt_for_that_turn] (1 message). The delta path fires.
/// Store grows by 1 per error turn:
///   - Turn 1 success: 2 messages (rig persists [user("t1"), assistant("ok")])
///   - Turn 2 error: +1 = 3 messages (delta = [user("t2")])
///   - Turn 3 error: +1 = 4 messages (delta = [user("t3")])
#[tokio::test]
async fn hard_error_twice_does_not_double_history() -> Result<()> {
    let config = test_config();
    let session_id = "test-no-double";
    let temp_dir = tempfile::tempdir().unwrap();

    // Turn 1: successful turn — rig appends [user("t1"), assistant("ok")] → store has 2 msgs.
    {
        let mut memory_state = make_memory_state(&temp_dir);
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
        let mut memory_state = make_memory_state(&temp_dir);
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
        let mut memory_state = make_memory_state(&temp_dir);
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

    // Final state: 2 (turn 1 success) + 1 (turn 2 user delta) + 1 (turn 3 user delta) = 4.
    // After the fix: on_completion_call stores history + [prompt], so delta = [user_prompt]
    // for each error turn. Delta path fires → 1 message per error turn, not 2 (no placeholder).
    let final_memory_state = make_memory_state(&temp_dir);
    let final_entries = final_memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let final_count: Vec<crate::types::Message> = final_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    let final_count = final_count.len();

    assert_eq!(
        final_count, 4,
        "two hard errors after one success must produce exactly 4 messages \
         (2 from turn1 + 1 user delta turn2 + 1 user delta turn3); got {} messages",
        final_count
    );
    assert!(
        final_count >= 2,
        "original turn 1 messages must be preserved; got {}",
        final_count
    );

    Ok(())
}
