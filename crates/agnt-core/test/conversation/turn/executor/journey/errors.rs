use super::*;

// ---------------------------------------------------------------------------
// Scenario 7: Repeated errors leave the store unchanged
// ---------------------------------------------------------------------------

/// rig saves a conversation only when a run succeeds, so a failed turn writes
/// nothing. One successful turn saves two messages; three failed turns after it
/// leave the store at two.
#[tokio::test]
async fn journey_repeated_errors_leave_the_store_unchanged() -> Result<()> {
    let mut h = JourneyHarness::new("journey-linear-errors");

    // Turn 1: success → 2 messages
    let (r1, _) = h
        .turn(
            "t1",
            MockCompletionModel::from_stream_turns([[
                MockStreamEvent::Text("ok".into()),
                MockStreamEvent::final_response_with_default_usage(),
            ]]),
            no_tools(),
        )
        .await;
    assert!(r1.is_ok(), "turn 1 must succeed: {r1:?}");
    assert_eq!(
        h.raw_messages().await?.len(),
        2,
        "after turn 1: expected 2 messages"
    );

    // Turns 2, 3, 4: error → each saves nothing
    for (i, prompt) in ["t2", "t3", "t4"].iter().enumerate() {
        let (r, _) = h
            .turn(
                prompt,
                MockCompletionModel::from_stream_turns([[MockStreamEvent::error(
                    "network timeout",
                )]]),
                no_tools(),
            )
            .await;
        assert!(r.is_err(), "turn {} must error", i + 2);
        assert_eq!(
            h.raw_messages().await?.len(),
            2,
            "after error turn {}: a failed turn saves nothing",
            i + 2
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 4: Error mid-tool-loop then successful recovery turn
// ---------------------------------------------------------------------------

/// Two-turn journey. Turn 1 runs two tool sub-turns and then fails on
/// sub-turn 3, so rig saves nothing. Turn 2 succeeds and starts from an empty
/// conversation: its request carries only the new prompt.
#[tokio::test]
async fn journey_error_mid_tool_loop_then_recovery() -> Result<()> {
    let mut h = JourneyHarness::new("journey-error-recovery");

    // Turn 1: tc1 executes, tc2 executes, sub-turn 3 errors (CompletionError)
    let model1 = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_echo", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::tool_call("tc2", "test_echo", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![MockStreamEvent::error("connection reset")],
    ]);
    let (r1, _) = h.turn("do things", model1, echo_tool("real_result")?).await;
    assert!(r1.is_err(), "CompletionError must propagate");

    // A failed run saves nothing — the real tool results are not persisted.
    let after_failure = h.raw_messages().await?;
    assert!(
        after_failure.is_empty(),
        "a failed turn must save nothing; got: {after_failure:?}"
    );

    // Turn 2: recovery — plain text, starts from an empty conversation
    let model2 = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("recovered".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let model2_spy = model2.clone();
    let (r2, _) = h.turn("continue", model2, no_tools()).await;
    assert!(r2.is_ok(), "recovery turn must succeed: {r2:?}");

    let msgs = h.raw_messages().await?;
    assert_eq!(
        msgs.len(),
        2,
        "the recovery turn saves its own exchange; got: {msgs:?}"
    );
    assert_user_text(&msgs[0], "continue");
    assert_assistant_text_contains(&msgs[1], "recovered");

    // The recovery request carries only the new prompt: the failed turn left
    // no history behind.
    assert_eq!(
        model2_spy.request_count(),
        1,
        "turn 2 model must have received exactly 1 request"
    );
    let req = &model2_spy.requests()[0];
    assert_eq!(
        req.chat_history.len(),
        1,
        "turn 2 chat_history must hold only the current prompt, got: {:?}",
        req.chat_history
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 5: Cancelled turn mid-tool-loop then continuation
// ---------------------------------------------------------------------------

/// Turn 1 executes a real tool call that cancels the turn from within `call()`.
/// A cancelled run saves nothing, so turn 2 starts from an empty conversation.
#[tokio::test]
async fn journey_cancelled_turn_then_continuation() -> Result<()> {
    let mut h = JourneyHarness::new("journey-cancel-tool");

    // Turn 1: the tool executes and cancels the turn from within call().
    let bus = crate::bus::create_bus();
    let model1 = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call(
                "tc1",
                "nu__shell",
                serde_json::json!({"command": "git pull"}),
            ),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let (r1, _) = h
        .turn(
            "run a git command",
            model1,
            nu_shell_cancelling_tool("Already up to date.", bus)?,
        )
        .await;
    assert!(r1.is_ok(), "cancelled turn must return Ok: {r1:?}");
    let r1 = r1.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(matches!(r1, TurnOutcome::EarlyReturn(_)));

    let msgs = h.raw_messages().await?;
    assert!(
        msgs.is_empty(),
        "a cancelled turn must save nothing; got: {msgs:?}"
    );

    // Turn 2: continuation starts from an empty conversation
    let model2 = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("I can see the git pull completed. How can I help next?".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let model2_spy = model2.clone();
    let (r2, _) = h.turn("what happened?", model2, no_tools()).await;
    assert!(r2.is_ok());
    assert_eq!(h.raw_messages().await?.len(), 2);
    let requests = model2_spy.requests();
    // The cancelled turn left no history, so the request carries only the
    // current prompt.
    assert_eq!(
        requests[0].chat_history.len(),
        1,
        "turn 2 chat_history must hold only the current prompt, got: {:?}",
        requests[0].chat_history
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 6: Session reload from disk — fresh MemoryState loads prior JSONL
// ---------------------------------------------------------------------------

/// Two separate `MemoryState` instances on the same tempdir path.
/// Verifies that a fresh `MemoryState` (simulating a new CLI invocation) correctly
/// loads the prior conversation from the rig store and passes it as context to
/// the LLM on the second turn.
///
/// Does NOT use `JourneyHarness` — uses `TurnExecutor` directly to keep two
/// separate `MemoryState` lifetimes.
#[tokio::test]
async fn journey_session_reload_from_disk() -> Result<()> {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let path = temp_dir.path().to_path_buf();
    let session_id = "journey-reload";
    let config = test_config();

    // Turn 1 — MemoryState A: write 2 messages to disk then drop.
    let spy1 = {
        let mut ms = memory_state_at(path.clone());
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("turn1 response".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ]]);
        let shared_model = test_utils::shared_model_handle(model.clone());
        let spy = model.clone();
        let mut executor = TurnExecutor::new(
            &config,
            &mut ms,
            no_tools(),
            shared_model,
            test_utils::test_compaction_config(crate::bus::create_bus()),
        );
        let r = executor
            .execute(
                ExecuteInput {
                    prompt: "turn1".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
        assert!(r.is_ok(), "turn 1 must succeed");
        spy
    }; // ms dropped — only JSONL remains on disk

    // Turn 1's model received 1 request; its chat_history should have 0 prior messages
    // (it was the first turn).
    assert_eq!(spy1.request_count(), 1);

    // Turn 2 — fresh MemoryState B: must load 2 prior messages from disk.
    {
        let mut ms = memory_state_at(path.clone());
        let model2 = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("turn2 response".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ]]);
        let shared_model = test_utils::shared_model_handle(model2.clone());
        let spy2 = model2.clone();
        let mut executor = TurnExecutor::new(
            &config,
            &mut ms,
            no_tools(),
            shared_model,
            test_utils::test_compaction_config(crate::bus::create_bus()),
        );
        let r = executor
            .execute(
                ExecuteInput {
                    prompt: "turn2".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
        assert!(r.is_ok(), "turn 2 must succeed");

        // Verify the rig store: 2 from turn1 + 2 from turn2
        let store = ms
            .inner_memory()
            .rig_store(session_id)
            .map_err(|e| format!("rig store open: {e:?}"))?;
        let conversation =
            rig_memory::load(&*store, session_id).map_err(|e| format!("rig store load: {e:?}"))?;
        let msgs: Vec<Message> = conversation
            .messages()
            .iter()
            .map(|message| (**message).clone())
            .collect();
        assert_eq!(
            msgs.len(),
            4,
            "expected 4 messages (2 from turn1 + 2 from turn2), got: {msgs:?}"
        );
        assert_user_text(&msgs[0], "turn1");
        assert_user_text(&msgs[2], "turn2");

        // Verify rig loaded the prior messages + current prompt on turn 2's first request.
        // rig's CompletionRequestBuilder::build() pushes the current prompt into
        // chat_history before sending, so: 2 prior messages + 1 current "turn2" prompt = 3.
        assert_eq!(spy2.request_count(), 1);
        let req = &spy2.requests()[0];
        let prior: Vec<_> = req.chat_history.iter().collect();
        // 2 prior messages + 1 current "turn2" prompt = 3
        assert_eq!(
            prior.len(),
            3,
            "turn 2 chat_history must be 3 (2 prior + current prompt), got: {prior:?}"
        );
    }
    Ok(())
}
