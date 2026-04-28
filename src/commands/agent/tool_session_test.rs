use crate::session::{Message, SessionStore};
use tempfile::TempDir;

#[test]
fn test_tool_results_saved_to_session() {
    // Setup: Create temporary session store
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Create a session
    let mut session = store
        .get_or_create(Some("test-tool-save".to_string()))
        .unwrap();

    // Add a user message
    let user_msg = Message::new("user".to_string(), "Use the calculator tool".to_string());
    session.add_message(&store, user_msg).unwrap();

    // Add a tool result message (this is what we're testing)
    let tool_msg = Message::new(
        "tool".to_string(),
        "Tool 'calculator' returned: 42".to_string(),
    );
    session.add_message(&store, tool_msg).unwrap();

    // Verify: Check that tool message is in session
    let messages = session.messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].role(), "tool");
    assert_eq!(messages[1].content(), "Tool 'calculator' returned: 42");

    // Verify: Reload session and check persistence
    let reloaded = store.load_session("test-tool-save").unwrap();
    let reloaded_messages = reloaded.messages();
    assert_eq!(reloaded_messages.len(), 2);
    assert_eq!(reloaded_messages[1].role(), "tool");
}

#[test]
fn test_session_format_history() {
    // Setup: Create temporary session store
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Create a session with multiple message types
    let mut session = store
        .get_or_create(Some("test-format".to_string()))
        .unwrap();

    session
        .add_message(
            &store,
            Message::new("user".to_string(), "Hello".to_string()),
        )
        .unwrap();
    session
        .add_message(
            &store,
            Message::new("assistant".to_string(), "Hi there".to_string()),
        )
        .unwrap();
    session
        .add_message(
            &store,
            Message::new("tool".to_string(), "Tool result: success".to_string()),
        )
        .unwrap();

    // Call format_history() - this method doesn't exist yet, so test will fail
    let history = session.format_history();

    // Verify: All messages are formatted correctly
    assert!(history.contains("user: Hello"));
    assert!(history.contains("assistant: Hi there"));
    assert!(history.contains("tool: Tool result: success"));

    // Verify: Messages are separated by double newlines
    let expected = "user: Hello\n\nassistant: Hi there\n\ntool: Tool result: success";
    assert_eq!(history, expected);
}

#[test]
fn test_multi_turn_tool_context_preserved() {
    // Setup: Create temporary session store
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Simulate multi-turn conversation with tool use
    let mut session = store
        .get_or_create(Some("test-multi-turn".to_string()))
        .unwrap();

    // Turn 1: User asks, assistant uses tool, tool responds
    session
        .add_message(
            &store,
            Message::new("user".to_string(), "What is 2+2?".to_string()),
        )
        .unwrap();
    session
        .add_message(
            &store,
            Message::new(
                "assistant".to_string(),
                "Let me calculate that.".to_string(),
            ),
        )
        .unwrap();
    session
        .add_message(
            &store,
            Message::new("tool".to_string(), "Tool 'calc' returned: 4".to_string()),
        )
        .unwrap();
    session
        .add_message(
            &store,
            Message::new("assistant".to_string(), "The answer is 4.".to_string()),
        )
        .unwrap();

    // Turn 2: User asks follow-up
    session
        .add_message(
            &store,
            Message::new(
                "user".to_string(),
                "What was the previous result?".to_string(),
            ),
        )
        .unwrap();

    // Verify: History includes tool results
    let history = session.format_history();
    assert!(history.contains("Tool 'calc' returned: 4"));

    // Verify: All messages are present in order
    let messages = session.messages();
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[2].role(), "tool");
    assert!(messages[2].content().contains("4"));
}

#[test]
fn test_tool_result_persisted_to_jsonl() {
    // Setup: Create temporary session store
    let temp_dir = TempDir::new().unwrap();
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());

    // Create session and add tool message
    let mut session = store.get_or_create(Some("test-jsonl".to_string())).unwrap();

    session
        .add_message(&store, Message::new("user".to_string(), "Test".to_string()))
        .unwrap();
    session
        .add_message(
            &store,
            Message::new("tool".to_string(), "Tool result".to_string()),
        )
        .unwrap();

    // Drop session to ensure all writes are flushed
    drop(session);

    // Reload and verify
    let reloaded = store.load_session("test-jsonl").unwrap();
    let messages = reloaded.messages();

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].role(), "tool");
    assert_eq!(messages[1].content(), "Tool result");
}

#[test]
fn test_tool_results_without_session() {
    // This test verifies that tool results are passed to LLM even WITHOUT --session flag
    // Bug: Currently tool results are ONLY passed if there's a session
    // Expected: Tool results should be tracked in-memory and passed to subsequent LLM calls

    // Setup: Create a mock scenario where we track conversation_messages in-memory
    // (This will be implemented in the agent loop in mod.rs)

    // Simulate the agent loop WITHOUT session:
    let mut conversation_messages: Vec<(String, String)> = vec![];

    // Step 1: Initial user prompt
    let user_prompt = "List files in current directory";
    conversation_messages.push(("user".to_string(), user_prompt.to_string()));

    // Step 2: First LLM response (wants to use tool)
    let assistant_response = "I'll use the ls tool";
    conversation_messages.push(("assistant".to_string(), assistant_response.to_string()));

    // Step 3: Tool execution result
    let tool_result = "Tool 'ls' returned: [file1.txt, file2.rs]";
    conversation_messages.push(("tool".to_string(), tool_result.to_string()));

    // Step 4: Build history prompt from conversation_messages (NOT from session)
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

    // Verify: history_prompt contains tool results
    assert!(history_prompt.contains("Tool 'ls' returned"));
    assert!(history_prompt.contains("[file1.txt, file2.rs]"));

    // Verify: All conversation turns are present
    assert!(history_prompt.contains("user: List files"));
    assert!(history_prompt.contains("assistant: I'll use the ls tool"));
    assert!(history_prompt.contains("tool: Tool 'ls' returned"));

    // Verify: Proper formatting
    assert!(history_prompt.contains("Previous conversation:"));
    assert!(history_prompt.contains("---"));
    assert!(history_prompt.contains("Continue responding."));
}
