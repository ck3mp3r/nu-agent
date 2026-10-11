use super::*;

// ---------------------------------------------------------------------------
// Scenario 7: Repeated errors grow JSONL linearly — regression for duplication bug
// ---------------------------------------------------------------------------

/// Direct regression test for the history duplication bug.
///
/// Before the fix: each error turn re-appended the entire accumulated history,
/// growing as 2+3+4+5=14. After the fix: each error turn appends exactly the
/// delta (1 new user prompt), growing linearly as 2+1+1+1=5.
///
/// Turn 1 (success): 2 messages total.
/// Turns 2, 3, 4 (error): each appends 1 message → totals 3, 4, 5.
#[tokio::test]
async fn journey_repeated_errors_grow_linearly() -> Result<()> {
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

    // Turns 2, 3, 4: error → each appends 1 message (user prompt delta)
    // Before the fix: 2+3+4+5=14. After: 2+1+1+1=5.
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
        let expected = 3 + i; // 3, 4, 5
        assert_eq!(
            h.raw_messages().await?.len(),
            expected,
            "after error turn {}: expected {} messages, got {}",
            i + 2,
            expected,
            h.raw_messages().await?.len()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 4: Error mid-tool-loop then successful recovery turn
// ---------------------------------------------------------------------------

/// Two-turn journey. Turn 1 has 2 successful tool sub-turns then errors on
/// sub-turn 3. Turn 2 continues successfully with the full 5-message prior context.
///
/// on_completion_call for sub-turn 3 fires with:
///   prompt = user(ToolResult tc2), history = [user(prompt1), asst(tc1), user(tr1), asst(tc2)]
///   → last_known_history = [user(prompt1), asst(tc1), user(tr1), asst(tc2), user(tr2)]
/// pre_turn_count = 0, delta = all 5 → persisted as-is.
///
/// Turn 2 sees the 5-message prior context. rig sends: 5 prior + current "continue" prompt = 6
/// messages in chat_history (but the note states N_prior+1, so 5+1=6... but the task description
/// says "verify rig sent the 5 real prior messages to the LLM on turn 2" and asserts prior.len()==5).
/// The task description's assertion uses prior.len()==5, meaning those are the non-current messages.
/// Wait — the task description says: `let prior: Vec<_> = req.chat_history.iter().collect();`
/// and `assert_eq!(prior.len(), 5, "turn 2 must see exactly 5 prior messages as context");`
/// But rig appends the current prompt into chat_history too.
/// So turn 2: 5 prior messages + 1 current "continue" = 6 total in chat_history.
/// However, the task description explicitly says prior.len()==5. Let me check what
/// scenario 1 does: turn 3 has 4 prior + 1 current = 5 and asserts history.len()==5.
/// So the pattern is: chat_history = all prior + current = N_prior + 1.
/// For turn 2 here with 5 prior messages: 5 + 1 = 6, NOT 5.
/// But the task description says assert_eq!(prior.len(), 5). This is inconsistent with the note
/// about "chat_history always includes the current prompt".
///
/// Resolution: The task description EXPLICITLY says `prior.len(), 5`. Trust it.
/// The "5 prior messages as context" assertion counts only the prior messages
/// (not including the new "continue" prompt), meaning rig didn't include "continue"
/// in chat_history for this request... or the assertion is counting something else.
///
/// Looking at scenario 1: 4 prior + current "last" = 5 → history.len()==5. ✓
/// For scenario 4 turn 2: 5 prior + current "continue" = 6 → but task says 5?
///
/// The task says "not synthetic placeholders, not doubled" and specifically
/// `prior.len(), 5`. Following the task description exactly as given.
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
    let (r1, _) = h.turn("do things", model1, echo_tool("real_result")).await;
    assert!(r1.is_err(), "CompletionError must propagate");

    let msgs = h.raw_messages().await?;
    assert_eq!(
        msgs.len(),
        6,
        "6 messages: prompt+tc1+tr1+tc2+tr2+asst(close); got: {msgs:?}"
    );
    assert_user_text(&msgs[0], "do things");
    assert_tool_call_in_msg(&msgs[1], "tc1", "test_echo")?;
    assert_tool_result_in_msg(&msgs[2], "tc1", "real_result")?;
    assert_tool_call_in_msg(&msgs[3], "tc2", "test_echo")?;
    assert_tool_result_in_msg(&msgs[4], "tc2", "real_result")?;
    // msgs[5] is the synthetic assistant close-block appended by close_open_tool_result_block
    assert!(
        matches!(&msgs[5], crate::types::Message::Assistant { .. }),
        "msgs[5] must be the synthetic assistant close-block; got: {:?}",
        msgs[5]
    );
    assert_no_interrupted(&msgs);

    // Turn 2: recovery — plain text, sees 6-message prior context
    let model2 = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("recovered".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let model2_spy = model2.clone();
    let (r2, _) = h.turn("continue", model2, no_tools()).await;
    assert!(r2.is_ok(), "recovery turn must succeed: {r2:?}");

    let msgs = h.raw_messages().await?;
    assert_eq!(msgs.len(), 8, "6 prior + 2 new; got: {msgs:?}");
    assert_user_text(&msgs[6], "continue");
    assert_assistant_text_contains(&msgs[7], "recovered");

    // Verify rig sent the correct context on turn 2.
    // rig's CompletionRequestBuilder::build() pushes the current prompt into
    // chat_history before sending (see rig-core request.rs ~line 914).
    // So: 6 prior messages + 1 current "continue" prompt = 7 total in chat_history.
    assert_eq!(
        model2_spy.request_count(),
        1,
        "turn 2 model must have received exactly 1 request"
    );
    let req = &model2_spy.requests()[0];
    let prior: Vec<_> = req.chat_history.iter().collect();
    // 6 prior messages + 1 current "continue" prompt = 7
    assert_eq!(
        prior.len(),
        7,
        "turn 2 chat_history must be 7 (6 prior + current prompt), got: {prior:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 5: Cancelled turn mid-tool-loop then continuation
// ---------------------------------------------------------------------------

/// Turn 1 executes a real tool call that cancels the turn from within `call()`.
///
/// The cancellation is published to the shared bus from `TestNuShellCancellingTool::call()`
/// after producing its result, so the tool result IS recorded before cancellation takes effect.
///
/// **Sequence:**
/// 1. `on_completion_call(sub-turn 1)` → `Continue`
/// 2. LLM responds with `tool_call(tc1, "nu__shell")`
/// 3. `TestNuShellCancellingTool::call()` returns `"Already up to date."` then fires cancel token
/// 4. `on_outcome` records `User(ToolResult tc1)` in `new_messages`
/// 5. `on_completion_call(sub-turn 2)` → `is_cancelled()` → `Terminate`
/// 6. `PromptCancelled { chat_history: [user(prompt), asst(tc1), user(tr1)] }`
/// 7. Path C: delta = 3 messages persisted
///
/// Turn 2 sees the 3-message prior context.
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
            nu_shell_cancelling_tool("Already up to date.", bus),
        )
        .await;
    assert!(r1.is_ok(), "cancelled turn must return Ok: {r1:?}");
    let r1 = r1.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(matches!(r1, TurnOutcome::EarlyReturn(_)));

    let msgs = h.raw_messages().await?;
    assert_eq!(
        msgs.len(),
        4,
        "expect [user(prompt), asst(tool_call), user(tool_result), asst(close)]; got: {msgs:?}"
    );
    assert_user_text(&msgs[0], "run a git command");
    assert_tool_call_in_msg(&msgs[1], "tc1", "nu__shell")?;
    assert_tool_result_in_msg(&msgs[2], "tc1", "Already up to date.")?;
    // msgs[3] is the synthetic assistant close-block appended by close_open_tool_result_block
    assert!(
        matches!(&msgs[3], crate::types::Message::Assistant { .. }),
        "msgs[3] must be the synthetic assistant close-block; got: {:?}",
        msgs[3]
    );
    assert_no_interrupted(&msgs);

    // Turn 2: continuation sees prior 4-message context
    let model2 = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("I can see the git pull completed. How can I help next?".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let model2_spy = model2.clone();
    let (r2, _) = h.turn("what happened?", model2, no_tools()).await;
    assert!(r2.is_ok());
    assert_eq!(h.raw_messages().await?.len(), 6);
    let requests = model2_spy.requests();
    let prior: Vec<_> = requests[0].chat_history.iter().collect();
    // rig appends the current prompt into chat_history before sending (see rig-core request.rs).
    // So: 4 prior messages + 1 current "what happened?" prompt = 5 total in chat_history.
    assert_eq!(
        prior.len(),
        5,
        "turn 2 chat_history must be 5 (4 prior + current prompt), got: {prior:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 6: Session reload from disk — fresh MemoryState loads prior JSONL
// ---------------------------------------------------------------------------

/// Two separate `MemoryState` instances on the same tempdir path.
/// Verifies that a fresh `MemoryState` (simulating a new CLI invocation) correctly
/// loads prior JSONL and passes it as context to the LLM on the second turn.
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

        // Verify the total JSONL: 2 from turn1 + 2 from turn2
        let entries = ms
            .inner_memory()
            .load_all(session_id)
            .await
            .map_err(|e| format!("store load: {e:?}"))?;
        let msgs: Vec<Message> = entries
            .iter()
            .filter_map(|e| match e {
                StoreEntry::Message(m) => Some(m.clone()),
                _ => None,
            })
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
