//! History-repair tests: inject_missing_tool_results + close_open_tool_result_block.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_compaction_config, test_config};
use super::*;
use crate::session::StoreEntry;

// ---------------------------------------------------------------------------
// Subtask 1 — inject_missing_tool_results: integration tests
// ---------------------------------------------------------------------------

/// On PromptCancelled (Ok path, cancelled=true) with an unpaired ToolCall in
/// the chat_history, the messages written to JSONL must contain a synthetic
/// User(ToolResult) for that ToolCall ID so the stored history is always valid.
///
/// We force a PromptCancelled scenario by triggering immediate cancellation.
/// The mock model emits a tool_call on its first (and only) turn; the UI
/// cancels immediately so rig returns PromptCancelled with a chat_history that
/// contains the Assistant(ToolCall) but no User(ToolResult).
///
/// After inject_missing_tool_results, the persisted JSONL must contain both
/// the Assistant(ToolCall) and a User(ToolResult{id, content:"[interrupted]"}).
#[tokio::test]
async fn prompt_cancelled_with_unpaired_tool_call_injects_synthetic_result() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-cancel-inject";
    let mut memory_state = make_memory_state(&temp_dir);

    // Model issues a tool call to the cancelling tool — the cancel fires after
    // the tool result is produced but before the next on_completion_call, so
    // chat_history will contain Assistant(ToolCall) with no matching
    // User(ToolResult), exercising the synthetic-result injection.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc_cancel_1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    // The cancelling tool publishes a CancelEvent from inside `call()` (after
    // the hook subscribes to `bus.cancel()`), driving the cancelled path
    // deterministically.
    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "test_cancel_tool".to_string(),
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "call a tool".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_ok(), "cancelled turn must not return Err");
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));

    // The persisted JSONL must contain both ToolCall and ToolResult entries.
    // If the model was fast enough that the tool call was actually processed
    // before cancel fired, we may get a completed turn. Either way, if a
    // ToolCall was persisted, its ToolResult must also be persisted.
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

    // For every Assistant message with a ToolCall, there must be a following
    // User message with the matching ToolResult.
    use crate::types::{AssistantContent, UserContent};
    for (i, msg) in persisted.iter().enumerate() {
        let crate::types::Message::Assistant { content, .. } = msg else {
            continue;
        };
        for item in content.iter() {
            let AssistantContent::ToolCall(tc) = item else {
                continue;
            };
            let call_id = &tc.id;
            // Find the next message
            let next_has_result = persisted.get(i + 1).is_some_and(|next| {
                if let crate::types::Message::User { content } = next {
                    content.iter().any(|item| {
                        if let UserContent::ToolResult(tr) = item {
                            &tr.call == call_id
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            });
            assert!(
                next_has_result,
                "ToolCall id={call_id} must have a matching ToolResult in the persisted history"
            );
        }
    }

    Ok(())
}

/// On UnknownToolCall (Err path, e.messages=Some) with an unpaired ToolCall in
/// chat_history, the messages persisted to JSONL must contain a synthetic
/// User(ToolResult) immediately after the unpaired Assistant(ToolCall).
#[tokio::test]
async fn unknown_tool_error_with_unpaired_tool_call_injects_synthetic_result() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-unknown-inject";
    let mut memory_state = make_memory_state(&temp_dir);

    // Model calls a tool that is not registered — triggers UnknownToolCall.
    // The chat_history will contain the user prompt + Assistant(ToolCall) but
    // no User(ToolResult) since the tool could not be dispatched.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call(
            "tc_unknown_1",
            "nonexistent_tool",
            serde_json::json!({"arg": "value"}),
        ),
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
                prompt: "use a tool".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // UnknownToolCall returns Err
    assert!(result.is_err(), "UnknownToolCall must propagate as Err");

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

    // Same invariant: every persisted ToolCall must have an adjacent ToolResult.
    use crate::types::{AssistantContent, UserContent};
    for (i, msg) in persisted.iter().enumerate() {
        let crate::types::Message::Assistant { content, .. } = msg else {
            continue;
        };
        for item in content.iter() {
            let AssistantContent::ToolCall(tc) = item else {
                continue;
            };
            let call_id = &tc.id;
            let next_has_result = persisted.get(i + 1).is_some_and(|next| {
                if let crate::types::Message::User { content } = next {
                    content.iter().any(|item| {
                        if let UserContent::ToolResult(tr) = item {
                            &tr.call == call_id
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            });
            assert!(
                next_has_result,
                "ToolCall id={call_id} must have a matching ToolResult in the persisted history (UnknownToolCall path)"
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// close_open_tool_result_block unit tests
// ---------------------------------------------------------------------------

/// Helper: build a User message whose content is a single ToolResult.
fn user_with_tool_result(id: &str) -> crate::types::Message {
    crate::types::Message::User {
        content: vec![crate::types::UserContent::ToolResult(
            crate::types::ToolResult {
                call: crate::types::ToolCallId::new_or_mint(id),
                provider: None,
                name: "do_thing".into(),
                content: vec![crate::types::ToolResultContent::text("result")],
            },
        )],
    }
}

/// Helper: build an Assistant text message.
fn assistant_with_text(text: &str) -> crate::types::Message {
    crate::types::Message::assistant(text)
}

/// Helper: build a User message with mixed content (ToolResult + Text).
fn user_with_mixed_content(id: &str) -> crate::types::Message {
    crate::types::Message::User {
        content: vec![
            crate::types::UserContent::ToolResult(crate::types::ToolResult {
                call: crate::types::ToolCallId::new_or_mint(id),
                provider: None,
                name: "do_thing".into(),
                content: vec![crate::types::ToolResultContent::text("result")],
            }),
            crate::types::UserContent::Text(crate::types::Text::new("some text")),
        ],
    }
}

#[test]
fn close_open_tool_result_block_appends_when_last_is_tool_result() {
    use super::close_open_tool_result_block;

    let msgs = vec![
        user_with_text("prompt"),
        crate::types::Message::assistant("ok"),
        user_with_tool_result("tc1"),
    ];
    let result = close_open_tool_result_block(msgs, "server error");
    assert_eq!(result.len(), 4, "synthetic assistant must be appended");
    assert!(
        matches!(result[3], crate::types::Message::Assistant { .. }),
        "last message must be Assistant; got: {:?}",
        result[3]
    );
}

#[test]
fn close_open_tool_result_block_noop_when_last_is_assistant() {
    use super::close_open_tool_result_block;

    let msgs = vec![user_with_text("prompt"), assistant_with_text("response")];
    let len_before = msgs.len();
    let result = close_open_tool_result_block(msgs, "error");
    assert_eq!(result.len(), len_before, "no change when last is assistant");
    assert!(
        matches!(
            result[result.len() - 1],
            crate::types::Message::Assistant { .. }
        ),
        "last message must still be Assistant"
    );
}

#[test]
fn close_open_tool_result_block_noop_when_last_is_user_text() {
    use super::close_open_tool_result_block;

    let msgs = vec![
        user_with_text("prompt"),
        assistant_with_text("response"),
        user_with_text("follow up"),
    ];
    let len_before = msgs.len();
    let result = close_open_tool_result_block(msgs, "error");
    assert_eq!(result.len(), len_before, "no change when last is user text");
    assert!(
        matches!(result[result.len() - 1], crate::types::Message::User { .. }),
        "last message must still be User"
    );
}

#[test]
fn close_open_tool_result_block_noop_when_last_user_has_mixed_content() {
    use super::close_open_tool_result_block;

    let msgs = vec![
        user_with_text("prompt"),
        assistant_with_text("assistant"),
        user_with_mixed_content("tc1"),
    ];
    let len_before = msgs.len();
    let result = close_open_tool_result_block(msgs, "error");
    assert_eq!(
        result.len(),
        len_before,
        "no change when last user has mixed content (ToolResult + Text)"
    );
}
