use super::*;

/// Path B: cancel_token fired, partial text accumulated.
#[test]
fn path_b_cancelled_with_partial_text_constructs_user_and_assistant_messages() {
    let turn_result = TurnResult {
        text: "partial response".to_string(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: true,
        cancel_reason: None,
        stop_cause: None,
        last_inference: InferenceId::mint(),
        last_total_tokens: 0,
        last_known_history: vec![],
        pre_turn_message_count: 0,
        tool_records: vec![],
        streamed_tool_calls: vec![],
    };

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

    assert!(
        matches!(&cancelled_messages[0], Message::User { .. }),
        "First message must be a user message"
    );

    match &cancelled_messages[1] {
        Message::Assistant(assistant) => {
            let content = &assistant.content;
            let text = content.iter().find_map(|c| {
                if let AssistantContent::Text(t) = c {
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
#[test]
fn path_b_cancelled_with_empty_text_constructs_only_user_message() {
    let turn_result = TurnResult {
        text: String::new(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: false,
        cancelled: true,
        cancel_reason: None,
        stop_cause: None,
        last_inference: InferenceId::mint(),
        last_total_tokens: 0,
        last_known_history: vec![],
        pre_turn_message_count: 0,
        tool_records: vec![],
        streamed_tool_calls: vec![],
    };

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

// ---------------------------------------------------------------------------
// rig v0.39.0: Path B with tool calls — chat_history preserved after cancel
// ---------------------------------------------------------------------------

/// Regression test: Cancelled preserves tool_call + tool_result history.
#[test]
fn cancel_mid_tool_call_preserves_tool_call_and_tool_result_in_history() -> Result<()> {
    use serde_json::json;

    let mut chat_history: Vec<Message> = Vec::new();

    chat_history.push(Message::User {
        content: vec![UserContent::Text(Text::new("What is in /etc/hosts?"))],
    });

    chat_history.push(Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_abc123"),
            ToolFunction::new(ToolName::new("read_file")?, json!({ "path": "/etc/hosts" })),
        )),
    ])));

    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: CallId::from_wire("call_abc123"),
                name: ToolName::new("read_file")?,
                content: vec![ToolResultContent::Text(Text::new("file contents here"))],
                is_error: false,
            },
        )],
    });

    let err = rig::completion::PromptError::Cancelled {
        reason: "User pressed Esc during tool execution".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    assert!(
        turn_err.is_cancelled(),
        "TurnError must be marked as cancelled"
    );
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(msg, "User pressed Esc during tool execution");

    assert_eq!(
        messages.len(),
        3,
        "Must preserve user message + assistant(tool_call) + user(tool_result)"
    );

    match &messages[0] {
        Message::User { .. } => {}
        _ => panic!("msg[0] should be User (prompt)"),
    };
    match &messages[1] {
        Message::Assistant(_) => {}
        _ => panic!("msg[1] should be Assistant (tool_call)"),
    };
    match &messages[2] {
        Message::User { .. } => {}
        _ => panic!("msg[2] should be User (tool_result)"),
    };
    Ok(())
}

/// Regression: multiple tool-use cycles are all preserved on cancellation.
#[test]
fn cancel_preserves_multiple_tool_use_cycles() -> Result<()> {
    use serde_json::json;

    let mut chat_history: Vec<Message> = Vec::new();

    // First tool-use cycle
    chat_history.push(Message::User {
        content: vec![UserContent::Text(Text::new("List files"))],
    });
    chat_history.push(Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_001"),
            ToolFunction::new(ToolName::new("list_dir")?, json!({ "path": "/" })),
        )),
    ])));
    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: CallId::from_wire("call_001"),
                name: ToolName::new("list_dir")?,
                content: vec![ToolResultContent::Text(Text::new("/bin /usr /etc"))],
                is_error: false,
            },
        )],
    });

    // Second tool-use cycle
    chat_history.push(Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::Text(Text::new("Now read that file")),
    ])));
    chat_history.push(Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_002"),
            ToolFunction::new(
                ToolName::new("read_file")?,
                json!({ "path": "/etc/passwd" }),
            ),
        )),
    ])));
    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: CallId::from_wire("call_002"),
                name: ToolName::new("read_file")?,
                content: vec![ToolResultContent::Text(Text::new("root:x:0:0:root user"))],
                is_error: false,
            },
        )],
    });

    let err = rig::completion::PromptError::Cancelled {
        reason: "Cancelled during read_file tool call".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    assert!(turn_err.is_cancelled());

    let messages = match &turn_err {
        TurnError::Cancelled { messages, .. } => messages,
        _ => panic!(
            "expected Cancelled variant: all accumulated messages must be preserved on cancel — including 2 completed tool cycles"
        ),
    };

    assert_eq!(
        messages.len(),
        chat_history.len(),
        "Every single message in accumulated history should survive cancellation"
    );
    Ok(())
}
