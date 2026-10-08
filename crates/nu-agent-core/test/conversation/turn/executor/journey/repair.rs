use super::*;

// ---------------------------------------------------------------------------
// Gap 1B: corrupt session healed by repair on load
// ---------------------------------------------------------------------------

/// Verifies that a session corrupted before Gap 1A (user(ToolResult) → user(Text)
/// with no assistant between) is transparently healed on load and the next turn succeeds.
///
/// The repair happens inside `JournalConversationMemory::load()` via `repair_messages()`.
/// This test proves end-to-end that a corrupt session is healed and a new turn can succeed.
#[tokio::test]
async fn journey_corrupt_session_healed_by_repair_on_load() -> Result<()> {
    use std::io::Write;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let path = temp_dir.path().to_path_buf();
    let session_id = "journey-repair-load";
    let config = test_config();

    // Write corrupt JSONL directly to the session file using rig's own serialization.
    // The corrupt pattern: user(ToolResult) immediately followed by user(Text), no asst between.
    {
        use crate::types::{
            AssistantContent, CallId, Message, ToolCall, ToolFunction, ToolName, ToolResult,
            ToolResultContent, UserContent,
        };

        let session_file = path.join(format!("{}.jsonl", session_id));
        let mut f = std::fs::File::create(&session_file).expect("create session file");

        // metadata line (required by JsonlConversationStore::load as first line)
        let metadata = serde_json::json!({
            "type": "session",
            "session_id": session_id,
            "created_at": "2024-01-01T00:00:00Z"
        });
        writeln!(f, "{}", serde_json::to_string(&metadata).unwrap()).unwrap();

        // Build the corrupt messages using rig types and serialize them.
        // This ensures the JSONL format matches what rig can parse back.
        let corrupt_messages: Vec<Message> = vec![
            // user("run something")
            Message::user("run something"),
            // assistant(tool_call tc1 nu__shell)
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(ToolCall::new(
                    CallId::from_wire("tc1"),
                    ToolFunction::new(
                        ToolName::new("nu__shell").expect("non-empty tool name"),
                        serde_json::json!({"command": "git pull"}),
                    ),
                ))],
            },
            // user(tool_result tc1) — pure ToolResult message
            Message::User {
                content: vec![UserContent::ToolResult(ToolResult {
                    call: CallId::from_wire("tc1"),
                    name: ToolName::new("nu__shell").expect("non-empty tool name"),
                    content: vec![ToolResultContent::text("Already up to date.")],
                })],
            },
            // user("continue") — immediately after ToolResult, no assistant between them → CORRUPT
            Message::user("continue"),
        ];

        for msg in &corrupt_messages {
            writeln!(f, "{}", serde_json::to_string(msg).unwrap()).unwrap();
        }
    }

    // Now create a MemoryState pointing to the same directory (simulating a new CLI invocation
    // that loads the corrupt JSONL). Repair fires inside load().
    let mut ms = memory_state_at(path.clone());

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("The git pull succeeded.".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let shared_model = test_utils::shared_model_handle(model.clone());

    let mut executor = TurnExecutor::new(
        &config,
        &mut ms,
        no_tools(),
        shared_model,
        test_utils::test_compaction_config(crate::bus::create_bus()),
    );
    let outcome = executor
        .execute(
            ExecuteInput {
                prompt: "what happened?".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // The turn must succeed — repair healed the corrupt session on load.
    assert!(
        outcome.is_ok(),
        "turn must succeed after corrupt session is healed on load; got: {outcome:?}"
    );

    // Verify the raw JSONL contains the expected message count.
    // The corrupt JSONL had 4 messages. Repair is applied in-memory on load (not written
    // back to JSONL). The turn then appends 2 new messages (user prompt + assistant reply).
    // Raw JSONL = 4 original (corrupt) + 2 new = 6 total.
    let entries = ms
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load: {e:?}"))?;
    let raw: Vec<Message> = entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        raw.len(),
        6,
        "raw JSONL must have 6 messages (4 original + 2 new); got: {raw:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Gap 6: Pre-flight repair — empty ToolResult replaced with placeholder
// ---------------------------------------------------------------------------

/// Integration test: a session containing a ToolResult with empty content is
/// repaired ephemerally before the turn starts, and the turn succeeds.
///
/// The raw JSONL is NOT modified — the repair is applied only to the in-memory
/// cache so rig receives structurally valid history.
///
/// Setup:
/// 1. Write a valid session (tool call + empty tool result + closing assistant)
///    directly into the memory cache to simulate a prior turn that produced
///    an empty tool result.
/// 2. Execute a follow-up turn — the pre-flight repair should replace the
///    empty ToolResult content with "(empty result)" before rig sees the history.
/// 3. Assert the turn succeeds (Ok).
/// 4. Assert the in-memory history (post-turn raw messages) contains "(empty result)".
#[tokio::test]
async fn journey_empty_tool_result_replaced_with_placeholder() -> Result<()> {
    use crate::types::{
        AssistantContent, CallId, ToolCall, ToolFunction, ToolName, ToolResult, ToolResultContent,
        UserContent,
    };
    use rig::memory::ConversationMemory;

    let mut h = JourneyHarness::new("journey-gap6-empty-tool-result");

    // Pre-populate the in-memory cache with a valid-structured but empty-content
    // ToolResult message. This simulates a prior turn that stored an empty result.
    let tc_id = "tc_gap6";
    let prior_messages: Vec<crate::types::Message> = vec![
        crate::types::Message::user("run something"),
        crate::types::Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire(tc_id),
                ToolFunction::new(ToolName::new("test_echo")?, serde_json::json!({})),
            ))],
        },
        // Empty tool result — the key scenario for Gap 6
        crate::types::Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire(tc_id),
                name: ToolName::new("test_echo").expect("non-empty tool name"),
                content: vec![ToolResultContent::text("")],
            })],
        },
        crate::types::Message::assistant("[ok]"),
    ];

    h.memory_state
        .memory_mut()
        .append(&rig::id::ConversationId::from(h.session_id), prior_messages)
        .await
        .map_err(|e| format!("pre-populate cache: {e:?}"))?;

    // Now run a follow-up turn — pre-flight repair must fix the empty ToolResult.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("follow-up answer".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let (r, _) = h.turn("what was the result?", model, no_tools()).await;
    assert!(
        r.is_ok(),
        "turn must succeed after empty ToolResult is repaired; got: {r:?}"
    );

    // Verify the in-memory cache contains "(empty result)" for the repaired entry.
    let all_msgs = h
        .memory_state
        .memory_mut()
        .load(&rig::id::ConversationId::from(h.session_id))
        .await
        .map_err(|e| format!("load messages: {e:?}"))?;

    let has_placeholder = all_msgs.iter().any(|msg| {
        let crate::types::Message::User { content } = msg else {
            return false;
        };
        content.iter().any(|c| {
            match c {
            rig::message::UserContent::ToolResult(tr) => tr.content.iter().any(|tc| {
                matches!(tc, crate::types::ToolResultContent::Text(t) if t.text == "(empty result)")
            }),
            _ => false,
        }
        })
    });
    assert!(
        has_placeholder,
        "in-memory history must contain '(empty result)' placeholder after repair; msgs: {all_msgs:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Null-args ToolCall repair integration test
// ---------------------------------------------------------------------------

/// Verifies that a session containing a ToolCall with `arguments: null` (the poison
/// pattern from `on_invalid_tool_call` → Skip → rollback_messages) is transparently
/// healed on load and the next turn succeeds.
///
/// The repair happens inside `JournalConversationMemory::load()` via `repair_messages()`.
/// The `fix_null_tool_arguments` pass replaces `null` with `{}` before rig sees the history.
#[tokio::test]
async fn journey_null_args_tool_call_repaired_on_load() {
    use std::io::Write;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let path = temp_dir.path().to_path_buf();
    let session_id = "journey-null-args-repair";
    let config = test_config();

    // Write corrupt JSONL directly to the session file.
    // The corrupt pattern: ToolCall with `arguments: null` and its matching ToolResult.
    {
        use crate::types::{
            AssistantContent, CallId, Message, ToolCall, ToolFunction, ToolName, ToolResult,
            ToolResultContent, UserContent,
        };

        let session_file = path.join(format!("{session_id}.jsonl"));
        let mut f = std::fs::File::create(&session_file).expect("create session file");

        // metadata line (required by JsonlConversationStore::load as first line)
        let metadata = serde_json::json!({
            "type": "session",
            "session_id": session_id,
            "created_at": "2024-01-01T00:00:00Z"
        });
        writeln!(
            f,
            "{}",
            serde_json::to_string(&metadata).expect("serialize metadata")
        )
        .expect("write metadata");

        let corrupt_messages: Vec<Message> = vec![
            // user("run something")
            Message::user("run something"),
            // assistant(tool_call tc1 with null arguments) — THE POISON
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(ToolCall::new(
                    CallId::from_wire("tc_poison"),
                    ToolFunction::new(
                        ToolName::new("tmux__send_and_capture").expect("non-empty tool name"),
                        serde_json::Value::Null,
                    ),
                ))],
            },
            // user(tool_result tc1 — the Skip reason)
            Message::User {
                content: vec![UserContent::ToolResult(ToolResult {
                    call: CallId::from_wire("tc_poison"),
                    name: ToolName::new("tmux__send_and_capture").expect("non-empty tool name"),
                    content: vec![ToolResultContent::text("Tool not available")],
                })],
            },
            // assistant closing text
            Message::assistant("I see the tool is unavailable."),
        ];

        for msg in &corrupt_messages {
            writeln!(f, "{}", serde_json::to_string(msg).expect("serialize msg"))
                .expect("write msg");
        }
    }

    // Create a fresh MemoryState that loads the corrupt JSONL. Repair fires inside load().
    let mut ms = memory_state_at(path.clone());

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("The tool was unavailable but we can continue.".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let shared_model = test_utils::shared_model_handle(model.clone());

    let mut executor = TurnExecutor::new(
        &config,
        &mut ms,
        no_tools(),
        shared_model,
        test_utils::test_compaction_config(crate::bus::create_bus()),
    );
    let outcome = executor
        .execute(
            ExecuteInput {
                prompt: "what happened?".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // The turn must succeed — repair healed the null-args poison on load.
    assert!(
        outcome.is_ok(),
        "turn must succeed after null-args ToolCall is repaired on load; got: {outcome:?}"
    );
}
