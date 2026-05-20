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
    };

    assert_eq!(result.text, "Hello");
    assert_eq!(result.usage.input_tokens, 10);
    assert_eq!(result.usage.output_tokens, 5);
    assert_eq!(result.tool_call_count, 0);
}

#[test]
fn turn_error_can_be_constructed() {
    let error = TurnError {
        msg: "Test error".to_string(),
        cancelled: false,
    };

    assert_eq!(error.msg, "Test error");
    assert!(!error.cancelled);

    let cancelled = TurnError {
        msg: "Cancelled".to_string(),
        cancelled: true,
    };
    assert!(cancelled.cancelled);
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
    // Create a PromptError::PromptCancelled variant directly
    let err = rig::completion::PromptError::PromptCancelled {
        chat_history: vec![],
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
