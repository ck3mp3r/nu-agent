use super::*;

#[test]
fn turn_result_can_be_constructed() {
    let result = TurnResult {
        text: "Hello".to_string(),
        usage: rig::completion::request::Usage::new()
            .input_tokens(10)
            .output_tokens(5)
            .total_tokens(15)
            .cached_input_tokens(0)
            .cache_creation_input_tokens(0)
            .tool_use_prompt_tokens(0)
            .reasoning_tokens(0),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: false,
        cancelled: false,
        cancel_reason: None,
        stop_cause: None,
        last_inference: InferenceId::mint(),
        last_total_tokens: 0,
        last_known_history: vec![],
        pre_turn_message_count: 0,
        tool_records: vec![],
        streamed_tool_calls: vec![],
    };

    assert_eq!(result.text, "Hello");
    assert_eq!(result.usage.input_tokens, Some(10));
    assert_eq!(result.usage.output_tokens, Some(5));
    assert_eq!(result.tool_call_count, 0);
    assert!(!result.deltas_emitted);
    assert!(!result.cancelled);
}

#[test]
fn turn_error_can_be_constructed() {
    let error = TurnError::CompletionFailed {
        msg: "Test error".to_string(),
        kind: executor::CompletionErrorKind::Unknown,
        retry_after: None,
        exhaustion: None,
    };

    assert!(!error.is_cancelled());
    let msg = match &error {
        TurnError::CompletionFailed { msg, .. } => msg.as_str(),
        _ => panic!("expected CompletionFailed"),
    };
    assert_eq!(msg, "Test error");

    let cancelled = TurnError::Cancelled {
        msg: "Cancelled".to_string(),
        messages: vec![],
    };
    assert!(cancelled.is_cancelled());
}

#[test]
fn prompt_cancelled_error_is_detected_as_cancellation() {
    let user_msg = Message::User {
        content: vec![UserContent::Text(Text::new("Hello"))],
    };
    let err = rig::completion::PromptError::Cancelled {
        chat_history: vec![user_msg],
        reason: "Cancelled by user".to_string(),
    };

    let turn_err = TurnError::from(err);

    assert!(
        turn_err.is_cancelled(),
        "Cancelled variant should be detected as cancellation"
    );
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg, messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert!(msg.contains("Cancelled by user"));

    assert_eq!(
        messages.len(),
        1,
        "Should have one message from chat_history"
    );
}

#[test]
fn other_prompt_errors_are_not_cancelled() {
    let completion_err = rig::error::ProviderError::Response("Some other error".to_string());
    let err = rig::completion::PromptError::from(completion_err);

    let turn_err = TurnError::from(err);

    assert!(
        !turn_err.is_cancelled(),
        "Non-cancellation errors should not be detected as cancellation"
    );
}

#[test]
fn max_turns_error_is_not_cancelled() {
    let err = rig::completion::PromptError::MaxTurns {
        max_turns: 10,
        chat_history: vec![],
        prompt: Message::User {
            content: vec![UserContent::Text(Text::new("test"))],
        },
    };

    let turn_err = TurnError::from(err);

    assert!(
        !turn_err.is_cancelled(),
        "MaxTurns should not be detected as cancellation"
    );
}

/// Test that TurnContext can be created without an MCP runtime.
#[test]
fn turn_context_always_has_tool_server_handle() {
    let handle = rig::tool::server::ToolServer::new().run();
    let _handle_clone = handle.clone();
}

/// Test that a PromptError converts to TurnError with cancelled=true for cancel errors.
#[test]
fn prompt_error_from_cancelled_captures_messages() {
    let user_msg = Message::User {
        content: vec![UserContent::Text(Text::new("Tell me about async"))],
    };

    let prompt_err = rig::completion::PromptError::Cancelled {
        reason: "Hook cancelled".to_string(),
        chat_history: vec![user_msg],
    };

    let turn_err = TurnError::from(prompt_err);

    assert!(turn_err.is_cancelled());
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(msg, "Hook cancelled");
    assert_eq!(messages.len(), 1);
}

/// TurnError from Cancelled captures chat_history as messages.
#[test]
fn turn_error_from_prompt_cancelled_captures_messages() {
    let user_msg = Message::User {
        content: vec![UserContent::Text(Text::new("What is Rust?"))],
    };
    let assistant_msg = Message::Assistant(AssistantMessage::new(vec![AssistantContent::Text(
        Text::new("Rust is a systems programming..."),
    )]));

    let err = rig::completion::PromptError::Cancelled {
        reason: "User pressed Esc".to_string(),
        chat_history: vec![user_msg, assistant_msg],
    };

    let turn_err = TurnError::from(err);

    assert!(turn_err.is_cancelled());
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(msg, "User pressed Esc");

    assert_eq!(
        messages.len(),
        2,
        "Both user and assistant messages should be captured"
    );
}

#[test]
fn turn_error_from_non_cancelled_has_no_messages() {
    let completion_err = rig::error::ProviderError::Response("Network timeout".to_string());
    let err = rig::completion::PromptError::from(completion_err);

    let turn_err = TurnError::from(err);

    assert!(!turn_err.is_cancelled());
    assert!(
        !matches!(turn_err, TurnError::Cancelled { .. }),
        "Non-cancelled errors should not be Cancelled variant"
    );
}

#[test]
fn turn_result_cancelled_flag_propagates() {
    let cancelled_result = TurnResult {
        text: String::new(),
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
        cancel_reason: None,
        stop_cause: None,
        last_inference: InferenceId::mint(),
        last_total_tokens: 0,
        last_known_history: vec![],
        pre_turn_message_count: 0,
        tool_records: vec![],
        streamed_tool_calls: vec![],
    };

    assert!(
        !normal_result.cancelled,
        "Normal turn should not be cancelled"
    );
}
