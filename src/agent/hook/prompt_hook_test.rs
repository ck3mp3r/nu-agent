use super::*;
use rig::agent::PromptHook;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn make_hook() -> (CopilotPromptHook, mpsc::UnboundedReceiver<HookEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let token = CancellationToken::new();
    (CopilotPromptHook::new(tx, token), rx)
}

fn make_hook_with_token() -> (
    CopilotPromptHook,
    mpsc::UnboundedReceiver<HookEvent>,
    CancellationToken,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let token = CancellationToken::new();
    let hook = CopilotPromptHook::new(tx, token.clone());
    (hook, rx, token)
}

#[cfg(test)]
mod test_support {
    use rig::completion::request::{
        CompletionError, CompletionModel, CompletionRequest, CompletionResponse,
    };
    use rig::streaming::StreamingCompletionResponse;
    use serde::{Deserialize, Serialize};

    #[derive(Clone)]
    pub struct DummyModel;

    #[derive(Clone, Serialize, Deserialize)]
    pub struct DummyStreamResponse;

    impl rig::completion::request::GetTokenUsage for DummyStreamResponse {
        fn token_usage(&self) -> Option<rig::completion::request::Usage> {
            None
        }
    }

    impl CompletionModel for DummyModel {
        type Response = serde_json::Value;
        type StreamingResponse = DummyStreamResponse;
        type Client = ();

        fn make(_client: &(), _model: impl Into<String>) -> Self {
            Self
        }

        async fn completion(
            &self,
            _req: CompletionRequest,
        ) -> Result<CompletionResponse<Self::Response>, CompletionError> {
            unimplemented!()
        }

        async fn stream(
            &self,
            _req: CompletionRequest,
        ) -> Result<StreamingCompletionResponse<Self::StreamingResponse>, CompletionError> {
            unimplemented!()
        }
    }
}

use test_support::DummyModel;

#[tokio::test]
async fn on_tool_call_allowed_returns_continue() {
    let (hook, mut rx) = make_hook();

    // Spawn a task to respond to the permission request
    tokio::spawn(async move {
        if let Some(HookEvent::AskPermission { responder, .. }) = rx.recv().await {
            responder.send(PermissionDecision::Allow).unwrap();
        }
        // Consume ToolStart event
        rx.recv().await;
    });

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "read_file", None, "id1", "{}").await;
    assert!(matches!(result, ToolCallHookAction::Continue));
}

#[tokio::test]
async fn on_tool_call_denied_returns_skip() {
    let (hook, mut rx) = make_hook();

    tokio::spawn(async move {
        if let Some(HookEvent::AskPermission { responder, .. }) = rx.recv().await {
            responder.send(PermissionDecision::Deny).unwrap();
        }
    });

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "write_file", None, "id1", "{}").await;
    assert!(matches!(result, ToolCallHookAction::Skip { .. }));
}

#[tokio::test]
async fn on_tool_call_cancelled_returns_terminate() {
    let (hook, _rx, token) = make_hook_with_token();
    token.cancel();

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "read_file", None, "id1", "{}").await;
    assert!(matches!(result, ToolCallHookAction::Terminate { .. }));
}

#[tokio::test]
async fn doom_loop_triggers_terminate() {
    let (hook, mut rx) = make_hook();

    // Spawn task to auto-allow all permission requests
    let rx_handle = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let HookEvent::AskPermission { responder, .. } = event {
                let _ = responder.send(PermissionDecision::Allow);
            }
        }
    });

    // Call same tool with same args DOOM_LOOP_THRESHOLD times
    for i in 0..DOOM_LOOP_THRESHOLD {
        let result = PromptHook::<DummyModel>::on_tool_call(
            &hook,
            "read_file",
            None,
            &format!("id{i}"),
            "{\"path\": \"same\"}",
        )
        .await;
        if i < DOOM_LOOP_THRESHOLD - 1 {
            assert!(
                matches!(result, ToolCallHookAction::Continue),
                "iteration {i} should Continue"
            );
        } else {
            assert!(
                matches!(result, ToolCallHookAction::Terminate { .. }),
                "iteration {i} should Terminate"
            );
        }
    }

    rx_handle.abort();
}

#[tokio::test]
async fn on_completion_call_emits_llm_start() {
    let (hook, mut rx) = make_hook();
    use rig::completion::message::{Text, UserContent};
    use rig::message::Message;
    use rig::one_or_many::OneOrMany;

    let msg = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "hi".to_string(),
        })),
    };
    let result = PromptHook::<DummyModel>::on_completion_call(&hook, &msg, &[]).await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().unwrap();
    assert!(matches!(event, HookEvent::LlmStart));
}

#[tokio::test]
async fn on_text_delta_emits_delta_event() {
    let (hook, mut rx) = make_hook();

    let result = PromptHook::<DummyModel>::on_text_delta(&hook, "hello", "hello").await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().unwrap();
    match event {
        HookEvent::TextDelta { delta, aggregated } => {
            assert_eq!(delta, "hello");
            assert_eq!(aggregated, "hello");
        }
        _ => panic!("Expected TextDelta event"),
    }
}

#[tokio::test]
async fn on_text_delta_cancelled_returns_terminate() {
    let (hook, _rx, token) = make_hook_with_token();
    token.cancel();

    let result = PromptHook::<DummyModel>::on_text_delta(&hook, "hello", "hello").await;
    assert!(matches!(result, HookAction::Terminate { .. }));
}

#[tokio::test]
async fn on_tool_result_emits_tool_end() {
    let (hook, mut rx) = make_hook();

    let result = PromptHook::<DummyModel>::on_tool_result(
        &hook,
        "read_file",
        None,
        "id1",
        "{}",
        "file contents",
    )
    .await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().unwrap();
    match event {
        HookEvent::ToolEnd {
            name,
            success,
            result,
            ..
        } => {
            assert_eq!(name, "read_file");
            assert!(success);
            assert_eq!(result, "file contents");
        }
        _ => panic!("Expected ToolEnd event"),
    }
}

#[tokio::test]
async fn on_tool_call_includes_tool_call_id_in_ask_permission_event() {
    let (hook, mut rx) = make_hook();

    // Spawn a task to capture the AskPermission event and respond
    let handle = tokio::spawn(async move {
        if let Some(HookEvent::AskPermission {
            tool_call_id,
            responder,
            ..
        }) = rx.recv().await
        {
            // Verify the tool_call_id was passed through
            assert_eq!(tool_call_id, Some("call_xyz789".to_string()));
            responder.send(PermissionDecision::Allow).unwrap();
        } else {
            panic!("Expected AskPermission event");
        }
        // Consume ToolStart event
        rx.recv().await;
    });

    let result = PromptHook::<DummyModel>::on_tool_call(
        &hook,
        "read_file",
        Some("call_xyz789".to_string()),
        "internal_id",
        "{}",
    )
    .await;

    assert!(matches!(result, ToolCallHookAction::Continue));
    handle.await.unwrap();
}

#[tokio::test]
async fn on_tool_call_passes_none_when_tool_call_id_not_provided() {
    let (hook, mut rx) = make_hook();

    // Spawn a task to capture the AskPermission event and respond
    let handle = tokio::spawn(async move {
        if let Some(HookEvent::AskPermission {
            tool_call_id,
            responder,
            ..
        }) = rx.recv().await
        {
            // Verify None was passed through
            assert_eq!(tool_call_id, None);
            responder.send(PermissionDecision::Allow).unwrap();
        } else {
            panic!("Expected AskPermission event");
        }
        // Consume ToolStart event
        rx.recv().await;
    });

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "read_file", None, "internal_id", "{}").await;

    assert!(matches!(result, ToolCallHookAction::Continue));
    handle.await.unwrap();
}

// RED: Test for extracting token usage from completion response
#[tokio::test]
async fn on_completion_response_extracts_token_usage() {
    use rig::completion::message::{AssistantContent, Text, ToolCall, ToolFunction, UserContent};
    use rig::completion::request::{CompletionResponse, Usage};
    use rig::message::Message;
    use rig::one_or_many::OneOrMany;

    let (hook, mut rx) = make_hook();

    // Create a completion response with usage data and mixed content
    let usage = Usage {
        input_tokens: 150,
        output_tokens: 75,
        total_tokens: 225,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
    };

    let response_text = "Here is my response with some text.";
    let response = CompletionResponse {
        choice: OneOrMany::many(vec![
            AssistantContent::Text(Text {
                text: response_text.to_string(),
            }),
            AssistantContent::ToolCall(ToolCall::new(
                "call_1".to_string(),
                ToolFunction {
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({}),
                },
            )),
            AssistantContent::ToolCall(ToolCall::new(
                "call_2".to_string(),
                ToolFunction {
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({}),
                },
            )),
        ])
        .unwrap(),
        usage,
        raw_response: serde_json::json!({}),
        message_id: None,
    };

    let prompt = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "test prompt".to_string(),
        })),
    };

    let result = PromptHook::<DummyModel>::on_completion_response(&hook, &prompt, &response).await;
    assert!(matches!(result, HookAction::Continue));

    // Verify the emitted event has correct token counts
    let event = rx.try_recv().unwrap();
    match event {
        HookEvent::LlmEnd {
            response_chars,
            tool_calls,
            input_tokens,
            output_tokens,
            total_tokens,
        } => {
            assert_eq!(
                input_tokens, 150,
                "input_tokens should match response usage"
            );
            assert_eq!(
                output_tokens, 75,
                "output_tokens should match response usage"
            );
            assert_eq!(
                total_tokens, 225,
                "total_tokens should match response usage"
            );
            assert_eq!(
                response_chars,
                response_text.len(),
                "response_chars should count text length"
            );
            assert_eq!(tool_calls, 2, "should count 2 tool calls");
        }
        _ => panic!("Expected LlmEnd event"),
    }
}

#[tokio::test]
async fn on_completion_response_handles_text_only_response() {
    use rig::completion::message::{AssistantContent, Text, UserContent};
    use rig::completion::request::{CompletionResponse, Usage};
    use rig::message::Message;
    use rig::one_or_many::OneOrMany;

    let (hook, mut rx) = make_hook();

    let usage = Usage {
        input_tokens: 50,
        output_tokens: 25,
        total_tokens: 75,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
    };

    let response_text = "Simple text response.";
    let response = CompletionResponse {
        choice: OneOrMany::one(AssistantContent::Text(Text {
            text: response_text.to_string(),
        })),
        usage,
        raw_response: serde_json::json!({}),
        message_id: None,
    };

    let prompt = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "test".to_string(),
        })),
    };

    let result = PromptHook::<DummyModel>::on_completion_response(&hook, &prompt, &response).await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().unwrap();
    match event {
        HookEvent::LlmEnd {
            response_chars,
            tool_calls,
            input_tokens,
            output_tokens,
            total_tokens,
        } => {
            assert_eq!(input_tokens, 50);
            assert_eq!(output_tokens, 25);
            assert_eq!(total_tokens, 75);
            assert_eq!(response_chars, response_text.len());
            assert_eq!(tool_calls, 0, "text-only response should have 0 tool calls");
        }
        _ => panic!("Expected LlmEnd event"),
    }
}

#[tokio::test]
async fn on_completion_response_handles_tool_only_response() {
    use rig::completion::message::{AssistantContent, Text, ToolCall, ToolFunction, UserContent};
    use rig::completion::request::{CompletionResponse, Usage};
    use rig::message::Message;
    use rig::one_or_many::OneOrMany;

    let (hook, mut rx) = make_hook();

    let usage = Usage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
    };

    let response = CompletionResponse {
        choice: OneOrMany::one(AssistantContent::ToolCall(ToolCall::new(
            "call_1".to_string(),
            ToolFunction {
                name: "read_file".to_string(),
                arguments: serde_json::json!({}),
            },
        ))),
        usage,
        raw_response: serde_json::json!({}),
        message_id: None,
    };

    let prompt = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "test".to_string(),
        })),
    };

    let result = PromptHook::<DummyModel>::on_completion_response(&hook, &prompt, &response).await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().unwrap();
    match event {
        HookEvent::LlmEnd {
            response_chars,
            tool_calls,
            input_tokens,
            output_tokens,
            total_tokens,
        } => {
            assert_eq!(input_tokens, 100);
            assert_eq!(output_tokens, 50);
            assert_eq!(total_tokens, 150);
            assert_eq!(response_chars, 0, "tool-only response should have 0 chars");
            assert_eq!(tool_calls, 1, "should count 1 tool call");
        }
        _ => panic!("Expected LlmEnd event"),
    }
}
