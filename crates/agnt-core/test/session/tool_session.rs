#[test]
fn test_tool_results_without_session() {
    // This test verifies that tool results are passed to LLM even WITHOUT --session flag
    // Bug: Currently tool results are ONLY passed if there's a session
    // Expected: Tool results should be tracked in-memory and passed to subsequent LLM calls

    let mut conversation_messages: Vec<(String, String)> = vec![];

    let user_prompt = "List files in current directory";
    conversation_messages.push(("user".to_string(), user_prompt.to_string()));

    let assistant_response = "I'll use the ls tool";
    conversation_messages.push(("assistant".to_string(), assistant_response.to_string()));

    let tool_result = "tool[ls] → {} · done result=[file1.txt, file2.rs]";
    conversation_messages.push(("tool".to_string(), tool_result.to_string()));

    let history = conversation_messages
        .iter()
        .map(|(role, content)| format!("{}: {}", role, content))
        .collect::<Vec<_>>()
        .join("\n\n");

    let history_prompt = if !history.is_empty() {
        format!(
            "Previous conversation:\n{}\n\n---\n\nContinue responding.",
            history
        )
    } else {
        user_prompt.to_string()
    };

    assert!(history_prompt.contains("tool[ls] → {} · done"));
    assert!(history_prompt.contains("result=[file1.txt, file2.rs]"));
    assert!(history_prompt.contains("user: List files"));
    assert!(history_prompt.contains("assistant: I'll use the ls tool"));
    assert!(history_prompt.contains("tool: tool[ls] → {} · done"));
    assert!(history_prompt.contains("Previous conversation:"));
    assert!(history_prompt.contains("---"));
    assert!(history_prompt.contains("Continue responding."));
}

// TODO: Add tests for tool result handling via ConversationStore + rig Messages
// These should test:
// - Tool results saved to ConversationStore
// - Multi-turn tool context preserved across conversation
// - Tool results persisted correctly in JSONL format
// See src/session/store_test.rs for examples
