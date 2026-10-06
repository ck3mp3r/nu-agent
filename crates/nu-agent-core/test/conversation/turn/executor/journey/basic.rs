use super::*;

// ---------------------------------------------------------------------------
// Smoke test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journey_harness_smoke() -> Result<()> {
    let mut h = JourneyHarness::new("journey-smoke");
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("hi".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let (r, _) = h.turn("hello", model, no_tools()).await;
    assert!(r.is_ok(), "smoke turn must succeed: {r:?}");
    let msgs = h.raw_messages().await?;
    assert_eq!(msgs.len(), 2);
    assert_user_text(&msgs[0], "hello");
    assert_assistant_text_contains(&msgs[1], "hi");
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 1: Three sequential plain-text turns accumulate JSONL correctly
// ---------------------------------------------------------------------------

/// Verifies that three consecutive turns each contribute exactly [user, assistant]
/// to the session JSONL, growing the message count linearly: 2 → 4 → 6.
///
/// Also verifies that the mock model on turn 3 receives the 4 prior messages
/// as context (via `MockCompletionModel` clone spy pattern).
///
/// `MockCompletionModel` is `Clone` — it wraps `Arc<MockCompletionModelState>`,
/// so a clone shares the same internal state. Cloning before passing to `h.turn()`
/// lets us inspect `requests()` after the model is consumed.
#[tokio::test]
async fn journey_three_text_turns_accumulate_correctly() -> Result<()> {
    let mut h = JourneyHarness::new("journey-three-text");

    // Turn 1
    let model1 = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("hello back".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let (r1, _) = h.turn("hello", model1, no_tools()).await;
    assert!(r1.is_ok(), "turn 1 must succeed: {r1:?}");
    let msgs = h.raw_messages().await?;
    assert_eq!(msgs.len(), 2, "after turn 1: expected 2 messages");
    assert_user_text(&msgs[0], "hello");
    assert_assistant_text_contains(&msgs[1], "hello back");

    // Turn 2
    let model2 = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("world back".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let (r2, _) = h.turn("world", model2, no_tools()).await;
    assert!(r2.is_ok(), "turn 2 must succeed: {r2:?}");
    let msgs = h.raw_messages().await?;
    assert_eq!(msgs.len(), 4, "after turn 2: expected 4 messages");
    assert_user_text(&msgs[2], "world");
    assert_assistant_text_contains(&msgs[3], "world back");

    // Turn 3 — verify model receives the prior 4 messages as context
    // Clone before moving so we can inspect the shared state after `turn()` consumes it.
    let model3 = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("done".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let model3_spy = model3.clone();
    let (r3, _) = h.turn("last", model3, no_tools()).await;
    assert!(r3.is_ok(), "turn 3 must succeed: {r3:?}");
    assert_eq!(
        h.raw_messages().await?.len(),
        6,
        "after turn 3: expected 6 messages"
    );

    // Verify rig sent the correct context on turn 3's single request.
    // rig's CompletionRequestBuilder::build() pushes the current prompt into
    // chat_history before sending (see rig-core request.rs line ~914). So the
    // full chat_history = 4 prior messages + 1 current prompt = 5 total.
    assert_eq!(
        model3_spy.request_count(),
        1,
        "turn 3 model must have received exactly 1 request"
    );
    let req = &model3_spy.requests()[0];
    let history: Vec<_> = req.chat_history.iter().collect();
    // 4 prior messages + 1 current "last" prompt = 5
    assert_eq!(
        history.len(),
        5,
        "turn 3 chat_history must be 5 (4 prior + current prompt), got: {history:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 2: Two sequential tool calls then text — 6 JSONL messages
// ---------------------------------------------------------------------------

/// One `h.turn()` that drives three LLM sub-turns:
///   sub-turn 1: LLM emits tool_call(tc1) → echo tool executes → ToolResult injected
///   sub-turn 2: LLM emits tool_call(tc2) → echo tool executes → ToolResult injected
///   sub-turn 3: LLM emits text "Done, used both" → turn completes
///
/// Rig produces one `Message::Assistant` per sub-turn (no batching between sub-turns),
/// so the confirmed JSONL shape is 6 messages.
#[tokio::test]
async fn journey_two_sequential_tool_calls_then_text() -> Result<()> {
    let mut h = JourneyHarness::new("journey-two-tool-calls");

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_echo", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::tool_call("tc2", "test_echo", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("Done, used both".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let (r, _) = h.turn("do two things", model, echo_tool("result_42")).await;
    assert!(r.is_ok(), "turn must succeed: {r:?}");

    let msgs = h.raw_messages().await?;
    // Confirmed shape (rig produces one Assistant per sub-turn, no batching):
    // [0] User(prompt)
    // [1] Assistant(ToolCall tc1)
    // [2] User(ToolResult tc1 "result_42")
    // [3] Assistant(ToolCall tc2)
    // [4] User(ToolResult tc2 "result_42")
    // [5] Assistant(Text "Done, used both")
    assert_eq!(msgs.len(), 6, "expected 6 messages, got: {msgs:?}");
    assert_user_text(&msgs[0], "do two things");
    assert_tool_call_in_msg(&msgs[1], "tc1", "test_echo")?;
    assert_tool_result_in_msg(&msgs[2], "tc1", "result_42")?;
    assert_tool_call_in_msg(&msgs[3], "tc2", "test_echo")?;
    assert_tool_result_in_msg(&msgs[4], "tc2", "result_42")?;
    assert_assistant_text_contains(&msgs[5], "Done");
    assert_no_interrupted(&msgs);
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 3: Three batched tool calls in one LLM response — 4 JSONL messages
// ---------------------------------------------------------------------------

/// One `h.turn()` that drives two LLM sub-turns:
///   sub-turn 1: LLM emits three tool_calls in the same stream turn → rig batches
///               them into a single `Message::Assistant([tc1, tc2, tc3])`, then
///               executes all three and batches results into `Message::User([tr1, tr2, tr3])`
///   sub-turn 2: LLM emits text "All done" → turn completes
///
/// Because all three tool calls arrive in the same stream turn (before FinalResponse),
/// rig accumulates them into one assistant message and batches all results into one
/// user message. Total: 4 messages.
#[tokio::test]
async fn journey_three_batched_tool_calls_in_one_response() -> Result<()> {
    let mut h = JourneyHarness::new("journey-batched-tools");

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_echo", serde_json::json!({})),
            MockStreamEvent::tool_call("tc2", "test_echo", serde_json::json!({})),
            MockStreamEvent::tool_call("tc3", "test_echo", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("All done".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let (r, _) = h
        .turn("do three things", model, echo_tool("batch_result"))
        .await;
    assert!(r.is_ok(), "turn must succeed: {r:?}");

    let msgs = h.raw_messages().await?;
    // Shape: [User(prompt), Assistant([tc1,tc2,tc3]), User([tr1,tr2,tr3]), Assistant(text)]
    // All three tool calls arrive in the same stream turn → same Assistant message.
    // All three tool results are batched by rig into one User message.
    assert_eq!(msgs.len(), 4, "expected 4 messages, got: {msgs:?}");
    assert_user_text(&msgs[0], "do three things");
    // msgs[1] is Assistant with 3 ToolCall entries
    // msgs[2] is User with 3 ToolResult entries
    assert_assistant_text_contains(&msgs[3], "All done");
    assert_no_interrupted(&msgs);

    // Verify all three tool result IDs are present in the batched User message
    let Message::User { content } = &msgs[2] else {
        panic!("expected User message at index 2, got: {:?}", msgs[2]);
    };
    for id in ["tc1", "tc2", "tc3"] {
        assert!(
            content.iter().any(
                |c| matches!(c, rig::message::UserContent::ToolResult(tr) if tr.call.as_str() == id)
            ),
            "ToolResult {id} missing from batched user message; content: {content:?}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 3b: persisted tool-result success verdict (nu_agent_success)
// ---------------------------------------------------------------------------

/// A tool that always fails with a real execution error (io::Error). The
/// persisted failure text is rig's redacted model feedback for a source
/// error (`"the tool failed"`), which legacy text sniffing classifies as
/// SUCCESS — the persisted verdict flag is what fixes rehydration.
struct TestFailingTool;

impl rig::tool::Tool for TestFailingTool {
    const NAME: &'static str = "test_fail";
    type Error = std::io::Error;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Test tool that always fails".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}, "required": []})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Err(std::io::Error::other(
            "read failed: No such file or directory (os error 2)",
        ))
    }
}

/// Register one always-failing tool (test_fail).
fn failing_tool() -> ToolInfra {
    let handle = rig::tool::server::ToolServer::new()
        .tool(TestFailingTool)
        .run();
    default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: "test_fail".to_string(),
            description: "Test tool that always fails".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        }],
    )
}

/// A successful tool call must persist `nu_agent_success` = true on the
/// ToolResult's first Text block.
#[tokio::test]
async fn journey_successful_tool_call_persists_success_flag() -> Result<()> {
    let mut h = JourneyHarness::new("journey-verdict-success");

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_echo", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("done".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let (r, _) = h.turn("run it", model, echo_tool("result_42")).await;
    assert!(r.is_ok(), "turn must succeed: {r:?}");

    let msgs = h.raw_messages().await?;
    // Shape: [User(prompt), Assistant(tc1), User(tr1), Assistant(text)]
    assert_eq!(msgs.len(), 4, "expected 4 messages, got: {msgs:?}");
    assert_tool_result_in_msg(&msgs[2], "tc1", "result_42")?;
    assert_tool_result_flag(&msgs[2], "tc1", Some(true))?;
    Ok(())
}

/// A failing tool call (real execution error) must persist
/// `nu_agent_success` = false — the verdict, not the output text, decides
/// rehydration.
#[tokio::test]
async fn journey_failed_tool_call_persists_failure_flag() -> Result<()> {
    let mut h = JourneyHarness::new("journey-verdict-failure");

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_fail", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("recovered".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let (r, _) = h.turn("run it", model, failing_tool()).await;
    assert!(r.is_ok(), "tool failure must not fail the turn: {r:?}");

    let msgs = h.raw_messages().await?;
    // Shape: [User(prompt), Assistant(tc1), User(tr1), Assistant(text)]
    assert_eq!(msgs.len(), 4, "expected 4 messages, got: {msgs:?}");
    assert_tool_result_in_msg(&msgs[2], "tc1", "the tool failed")?;
    assert_tool_result_flag(&msgs[2], "tc1", Some(false))?;
    Ok(())
}

/// A sub-turn cap (max 1) skips the second tool call of the sub-turn: the
/// skipped call's persisted ToolResult must carry `nu_agent_success` = false
/// (Skipped disposition), while the executed first call carries true.
#[tokio::test]
async fn journey_subturn_cap_skipped_tool_call_persists_failure_flag() -> Result<()> {
    // -- Setup & Fixtures
    let mut config = test_config();
    config.max_tool_calls_per_subturn = Some(1);
    let mut h = JourneyHarness::new_with_config("journey-verdict-subturn-skip", config);

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_echo", serde_json::json!({})),
            MockStreamEvent::tool_call("tc2", "test_echo", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("done".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    // -- Exec
    let (r, _) = h.turn("run both", model, echo_tool("result_42")).await;
    assert!(r.is_ok(), "turn must succeed: {r:?}");

    // -- Check
    let msgs = h.raw_messages().await?;
    // Shape: [User(prompt), Assistant([tc1,tc2]), User([tr_tc1, tr_tc2]), Assistant(text)]
    assert_eq!(msgs.len(), 4, "expected 4 messages, got: {msgs:?}");
    assert_tool_result_in_msg(&msgs[2], "tc1", "result_42")?;
    assert_tool_result_flag(&msgs[2], "tc1", Some(true))?;
    // tc2 was skipped by the sub-turn cap — its persisted ToolResult carries
    // the skip reason and a false verdict.
    assert_tool_result_in_msg(&msgs[2], "tc2", "Sub-turn tool call limit reached")?;
    assert_tool_result_flag(&msgs[2], "tc2", Some(false))?;
    Ok(())
}
