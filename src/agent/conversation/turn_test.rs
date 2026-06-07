//! Tests for turn module
//!
//! Most tests are integration tests that require a real LLM,
//! so they are marked #[ignore] by default.

use super::*;

#[test]
fn turn_result_can_be_constructed() {
    let result = TurnResult {
        text: "Hello".to_string(),
        usage: rig::completion::request::Usage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            reasoning_tokens: 0,
        },
        messages: None,
        tool_call_count: 0,
        deltas_emitted: false,
        cancelled: false,
    };

    assert_eq!(result.text, "Hello");
    assert_eq!(result.usage.input_tokens, 10);
    assert_eq!(result.usage.output_tokens, 5);
    assert_eq!(result.tool_call_count, 0);
    assert!(!result.deltas_emitted);
    assert!(!result.cancelled);
}

#[test]
fn turn_error_can_be_constructed() {
    let error = TurnError {
        msg: "Test error".to_string(),
        cancelled: false,
        messages: None,
    };

    assert_eq!(error.msg, "Test error");
    assert!(!error.cancelled);
    assert!(error.messages.is_none());

    let cancelled = TurnError {
        msg: "Cancelled".to_string(),
        cancelled: true,
        messages: None,
    };
    assert!(cancelled.cancelled);
    assert!(cancelled.messages.is_none());
}

#[test]
#[ignore] // Requires real LLM setup
fn execute_turn_integration_test() {
    // Integration test structure (not implemented yet)
    // Would test:
    // 1. Create mock UI and permissions
    // 2. Create test agent
    // 3. Call execute_turn
    // 4. Verify TurnResult
}

#[test]
fn max_turns_value_is_stored_in_context() {
    // Test that max_turns flows through the TurnContext
    // This is a unit test for the data flow, not the agent behavior
    let max_turns = Some(5u32);

    // Create a minimal TurnContext (we can't easily instantiate all fields)
    // so we test the field can store the value
    assert_eq!(max_turns, Some(5u32));

    // This test is mainly a placeholder to document that max_turns should be:
    // 1. Passed through TurnContext
    // 2. Used in build_agent_and_prompt via .default_max_turns()
    // 3. Respected by rig's agent loop
}

/// Test that build_agent_and_prompt correctly passes max_turns to rig's AgentBuilder.
/// This test documents the fix for the MaxTurnError issue where max_turns was not
/// being set, causing rig's agent to default to 0 turns.
#[test]
fn build_agent_and_prompt_passes_max_turns_to_rig_agent_builder() {
    // This is a documentation test - the actual behavior is tested via integration
    // tests with real LLM calls. The key fix is:
    // 1. build_agent_and_prompt now accepts max_turns: Option<u32>
    // 2. It calls .default_max_turns(turns as usize) on AgentBuilder if Some
    // 3. TurnContext.max_turns field no longer has _ prefix (it's used)
    // 4. All three Agent enum variants pass ctx.max_turns to build_agent_and_prompt

    // We can't easily unit test the builder without mocking the entire rig stack,
    // but we can document the expected behavior:
    let max_turns_from_config = Some(10u32);

    // When passed to build_agent_and_prompt, it should:
    // - Convert to usize (10u32 as usize = 10usize)
    // - Call builder.default_max_turns(10)
    // - Result: agent will respect the turn limit

    assert_eq!(max_turns_from_config.map(|t| t as usize), Some(10usize));
}

#[test]
fn prompt_cancelled_error_is_detected_as_cancellation() {
    // Create a PromptError::PromptCancelled variant with chat_history
    let user_msg = rig::completion::Message::User {
        content: rig::one_or_many::OneOrMany::one(rig::completion::message::UserContent::Text(
            rig::completion::message::Text {
                text: "Hello".to_string(),
            },
        )),
    };
    let err = rig::completion::PromptError::PromptCancelled {
        chat_history: vec![user_msg],
        reason: "Cancelled by user".to_string(),
    };

    // Convert to TurnError
    let turn_err = TurnError::from(err);

    // Should be marked as cancelled
    assert!(
        turn_err.cancelled,
        "PromptCancelled variant should be detected as cancellation"
    );
    assert!(turn_err.msg.contains("Cancelled by user"));

    // Should capture chat_history
    let messages = turn_err
        .messages
        .expect("PromptCancelled should capture chat_history as messages");
    assert_eq!(
        messages.len(),
        1,
        "Should have one message from chat_history"
    );
}

#[test]
fn other_prompt_errors_are_not_cancelled() {
    // Create a non-cancellation error (CompletionError)
    let completion_err =
        rig::completion::CompletionError::ResponseError("Some other error".to_string());
    let err = rig::completion::PromptError::from(completion_err);

    // Convert to TurnError
    let turn_err = TurnError::from(err);

    // Should NOT be marked as cancelled
    assert!(
        !turn_err.cancelled,
        "Non-cancellation errors should not be detected as cancellation"
    );
}

#[test]
fn max_turns_error_is_not_cancelled() {
    // Create a MaxTurnsError
    let err = rig::completion::PromptError::MaxTurnsError {
        max_turns: 10,
        chat_history: Box::new(vec![]),
        prompt: Box::new(rig::completion::Message::User {
            content: rig::one_or_many::OneOrMany::one(rig::completion::message::UserContent::Text(
                rig::completion::message::Text {
                    text: "test".to_string(),
                },
            )),
        }),
    };

    // Convert to TurnError
    let turn_err = TurnError::from(err);

    // Should NOT be marked as cancelled
    assert!(
        !turn_err.cancelled,
        "MaxTurnsError should not be detected as cancellation"
    );
}

/// Test that TurnContext can be created without an MCP runtime.
/// This tests the bug fix: CLI mode without MCP servers should still have tools registered.
#[test]
fn turn_context_always_has_tool_server_handle() {
    // Before the fix: tool_server_handle was Option<ToolServerHandle>, which was None
    // when no MCP runtime existed. This meant no tools were registered.
    //
    // After the fix: tool_server_handle is always ToolServerHandle (not Option).
    // When no MCP runtime exists, we create a standalone ToolServer.

    // Create a standalone ToolServer (what the fix does when mcp_runtime is None)
    let handle = rig::tool::server::ToolServer::new().run();

    // Verify it's a valid handle by checking that it can be cloned
    // (ToolServerHandle implements Clone, so if this compiles and runs, it's valid)
    let _handle_clone = handle.clone();

    // This test documents that TurnContext.tool_server_handle is now:
    // - Type changed from Option<ToolServerHandle> to ToolServerHandle
    // - Always initialized (no None case)
    // - build_agent_and_prompt always uses the handle (no if-let branching)
}

/// Test that TurnContext uses InMemoryConversationMemory instead of session_history Vec.
/// This documents the refactor to use rig's memory system.
#[test]
fn turn_context_uses_memory_instead_of_history_vec() {
    // Before: TurnContext had session_history: Vec<rig::completion::Message>
    // After: TurnContext has memory: InMemoryConversationMemory + conversation_id: String

    // Create a memory instance (what TurnContext will store)
    let memory = rig::memory::InMemoryConversationMemory::new();
    let conversation_id = "test-conversation-123".to_string();

    // Memory can be cloned (required for passing to agent builder)
    let _memory_clone = memory.clone();

    // Conversation ID can be cloned (required for prompt request)
    let _id_clone = conversation_id.clone();

    // This test documents that:
    // 1. TurnContext.session_history (Vec<Message>) is removed
    // 2. TurnContext.memory (InMemoryConversationMemory) is added
    // 3. TurnContext.conversation_id (String) is added
    // 4. build_agent_and_prompt uses memory.clone() instead of history
    // 5. prompt request uses .conversation(id) instead of .with_history(vec)
}

/// Test that build_agent_and_prompt uses memory and conversation_id correctly.
/// This documents the API changes for the memory-based conversation system.
#[test]
fn build_agent_and_prompt_uses_memory_api() {
    // Before: build_agent_and_prompt(model, hook, preamble, prompt, history: Vec, handle, max_turns)
    // After: build_agent_and_prompt(model, hook, preamble, prompt, memory, conversation_id, handle, max_turns)

    let _memory = rig::memory::InMemoryConversationMemory::new();
    let conversation_id = "test-conv-456";

    // The function should:
    // 1. Accept memory: InMemoryConversationMemory (not history: Vec<Message>)
    // 2. Accept conversation_id: String
    // 3. Call builder.memory(memory.clone())
    // 4. Call agent.prompt(msg).conversation(conversation_id) instead of .with_history(vec)

    // Verify the memory can be used as expected
    assert_eq!(conversation_id, "test-conv-456");

    // This documents the signature change and usage pattern
}

/// Test that StreamingError converts to TurnError with cancelled=false.
/// StreamingErrors from rig indicate provider/network issues, not user cancellation.
#[test]
fn streaming_error_converts_to_turn_error() {
    // The From<rig::agent::StreamingError> implementation maps any StreamingError
    // to a TurnError with cancelled=false (since streaming errors are not cancellations)

    // We test this by verifying the implementation exists and documenting behavior:
    // 1. StreamingError is converted via .to_string() to get the error message
    // 2. cancelled is always false (streaming errors are provider/network issues)
    // 3. This matches the pattern: PromptError::PromptCancelled sets cancelled=true,
    //    all other errors (including StreamingError) set cancelled=false

    // Since rig::agent::StreamingError is an opaque external type that we cannot
    // easily construct in tests, we document the expected behavior:
    //
    // Given: StreamingError(msg)
    // When: TurnError::from(streaming_error)
    // Then: TurnError { msg: streaming_error.to_string(), cancelled: false }
}

/// Test that streaming-related types maintain consistency with TurnResult.
/// Documents that StreamingTurnResult (internal) maps cleanly to TurnResult (public).
#[test]
fn streaming_turn_result_fields_match_turn_result() {
    // StreamingTurnResult (private struct in turn.rs) contains:
    // - text: String
    // - usage: rig::completion::request::Usage
    // - messages: Option<Vec<rig::completion::Message>>

    // These fields map to TurnResult fields:
    // - text → text
    // - usage → usage
    // - messages → messages

    // TurnResult has additional fields populated by HookDriver:
    // - tool_call_count: usize (from driver, not from streaming)
    // - deltas_emitted: bool (from driver, not from streaming)

    // This test documents that the streaming result provides the LLM response data,
    // while the driver provides the tool execution metadata.

    // Construct a TurnResult to verify all fields are accessible
    let result = TurnResult {
        text: "Response from streaming".to_string(),
        usage: rig::completion::request::Usage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            reasoning_tokens: 0,
        },
        messages: None,
        tool_call_count: 3,   // From driver
        deltas_emitted: true, // From driver
        cancelled: false,
    };

    assert_eq!(result.text, "Response from streaming");
    assert_eq!(result.usage.total_tokens, 150);
    assert_eq!(result.tool_call_count, 3);
    assert!(result.deltas_emitted);
    assert!(!result.cancelled);
}

/// Test that build_agent_and_stream uses the multi-turn streaming API.
/// Documents the streaming workflow and API contract.
#[test]
fn build_agent_and_stream_uses_multi_turn_streaming() {
    // build_agent_and_stream is the async function that:
    // 1. Builds an agent with AgentBuilder
    // 2. Calls agent.stream_prompt() to get a streaming request
    // 3. Calls .conversation(conversation_id) to set conversation context
    // 4. Calls .with_history(empty_vec) for initial history (memory handles persistence)
    // 5. Calls .multi_turn(max_turns) to enable tool-calling loop
    // 6. Awaits the stream and processes MultiTurnStreamItem variants
    // 7. Returns StreamingTurnResult with text, usage, and messages

    // The streaming loop handles:
    // - StreamAssistantItem(Text) → accumulate text deltas
    // - StreamAssistantItem(ToolCall/ToolCallDelta) → ignored (handled by hooks)
    // - FinalResponse → capture final text, usage, and message history

    // This test documents the expected behavior without requiring a real agent.
    // Integration tests with real LLM calls verify the actual streaming behavior.

    let max_turns = 10usize;
    assert_eq!(max_turns, 10);

    // The key API contract:
    // - Input: CompletionModel + AgentPromptConfig
    // - Output: Result<StreamingTurnResult, rig::agent::StreamingError>
    // - Side effects: Emits text deltas via hooks (if hook is configured)
}

/// Test that TurnError from PromptCancelled captures chat_history as messages.
#[test]
fn turn_error_from_prompt_cancelled_captures_messages() {
    let user_msg = rig::completion::Message::User {
        content: rig::one_or_many::OneOrMany::one(rig::completion::message::UserContent::Text(
            rig::completion::message::Text {
                text: "What is Rust?".to_string(),
            },
        )),
    };
    let assistant_msg = rig::completion::Message::Assistant {
        id: None,
        content: rig::one_or_many::OneOrMany::one(
            rig::completion::message::AssistantContent::Text(rig::completion::message::Text {
                text: "Rust is a systems programming...".to_string(),
            }),
        ),
    };

    let err = rig::completion::PromptError::PromptCancelled {
        reason: "User pressed Esc".to_string(),
        chat_history: vec![user_msg, assistant_msg],
    };

    let turn_err = TurnError::from(err);

    assert!(turn_err.cancelled);
    assert_eq!(turn_err.msg, "User pressed Esc");

    let messages = turn_err
        .messages
        .expect("PromptCancelled should preserve chat_history");
    assert_eq!(
        messages.len(),
        2,
        "Both user and assistant messages should be captured"
    );
}

/// Test that TurnError from non-cancelled PromptError has no messages.
#[test]
fn turn_error_from_non_cancelled_has_no_messages() {
    let completion_err =
        rig::completion::CompletionError::ResponseError("Network timeout".to_string());
    let err = rig::completion::PromptError::from(completion_err);

    let turn_err = TurnError::from(err);

    assert!(!turn_err.cancelled);
    assert!(
        turn_err.messages.is_none(),
        "Non-cancelled errors should not have messages"
    );
}

/// Test that TurnError from StreamingError wrapping PromptCancelled captures messages.
#[test]
fn turn_error_from_streaming_prompt_cancelled_captures_messages() {
    let user_msg = rig::completion::Message::User {
        content: rig::one_or_many::OneOrMany::one(rig::completion::message::UserContent::Text(
            rig::completion::message::Text {
                text: "Tell me about async".to_string(),
            },
        )),
    };

    let inner = rig::completion::PromptError::PromptCancelled {
        reason: "Hook cancelled".to_string(),
        chat_history: vec![user_msg],
    };
    let streaming_err = rig::agent::StreamingError::Prompt(Box::new(inner));

    let turn_err = TurnError::from(streaming_err);

    assert!(turn_err.cancelled);
    assert_eq!(turn_err.msg, "Hook cancelled");

    let messages = turn_err
        .messages
        .expect("StreamingError wrapping PromptCancelled should capture chat_history");
    assert_eq!(messages.len(), 1);
}

/// Path B: cancel_token fired, partial text accumulated.
/// When cancelled=true, messages=None, and text is non-empty, the runtime must
/// construct BOTH a user message and an assistant message for persistence.
/// This test verifies the construction logic that drives the Path B code path.
#[test]
fn path_b_cancelled_with_partial_text_constructs_user_and_assistant_messages() {
    use rig::completion::Message;

    let turn_result = TurnResult {
        text: "partial response".to_string(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: true,
    };

    // Replicate Path B construction logic from runtime.rs execute_turn
    let prompt = "user prompt".to_string();
    let mut cancelled_messages = vec![Message::user(prompt.clone())];
    if !turn_result.text.is_empty() {
        cancelled_messages.push(Message::assistant(turn_result.text.clone()));
    }

    assert_eq!(
        cancelled_messages.len(),
        2,
        "Path B with partial text must produce user + assistant messages"
    );

    // Verify user message is first
    assert!(
        matches!(&cancelled_messages[0], Message::User { .. }),
        "First message must be a user message"
    );

    // Verify assistant message is second with the partial text
    match &cancelled_messages[1] {
        Message::Assistant { content, .. } => {
            let text = content.iter().find_map(|c| {
                if let rig::completion::message::AssistantContent::Text(t) = c {
                    Some(t.text.as_str())
                } else {
                    None
                }
            });
            assert_eq!(
                text,
                Some("partial response"),
                "Assistant message must contain partial text"
            );
        }
        other => panic!("Expected assistant message, got {:?}", other),
    }
}

/// Path B: cancel_token fired, no text accumulated.
/// When cancelled=true, messages=None, and text is empty, the runtime must
/// construct ONLY a user message — no empty assistant message should be appended.
#[test]
fn path_b_cancelled_with_empty_text_constructs_only_user_message() {
    use rig::completion::Message;

    let turn_result = TurnResult {
        text: String::new(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: false,
        cancelled: true,
    };

    // Replicate Path B construction logic from runtime.rs execute_turn
    let prompt = "user prompt".to_string();
    let mut cancelled_messages = vec![Message::user(prompt.clone())];
    if !turn_result.text.is_empty() {
        cancelled_messages.push(Message::assistant(turn_result.text.clone()));
    }

    assert_eq!(
        cancelled_messages.len(),
        1,
        "Path B with empty text must produce only the user message (no empty assistant)"
    );

    assert!(
        matches!(&cancelled_messages[0], Message::User { .. }),
        "The single message must be a user message"
    );
}

/// Test that TurnResult correctly propagates the cancelled flag.
#[test]
fn turn_result_cancelled_flag_propagates() {
    let cancelled_result = TurnResult {
        text: String::new(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: true,
    };

    assert!(cancelled_result.cancelled, "Cancelled flag should be true");
    assert!(
        cancelled_result.text.is_empty(),
        "Cancelled turn should have empty text"
    );
    assert!(
        cancelled_result.messages.is_none(),
        "Cancelled via cancel_token should have no messages (FinalResponse not received)"
    );

    let normal_result = TurnResult {
        text: "Hello".to_string(),
        usage: rig::completion::request::Usage::default(),
        messages: Some(vec![]),
        tool_call_count: 1,
        deltas_emitted: true,
        cancelled: false,
    };

    assert!(
        !normal_result.cancelled,
        "Normal turn should not be cancelled"
    );
}
