use super::*;
use crate::types::{Text, UserContent};
use rig::agent::PromptHook;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn make_hook() -> (CopilotPromptHook, mpsc::UnboundedReceiver<HookEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let token = CancellationToken::new();
    let last_total_tokens = Arc::new(AtomicU64::new(0));
    (CopilotPromptHook::new(tx, token, last_total_tokens), rx)
}

fn make_hook_with_token() -> (
    CopilotPromptHook,
    mpsc::UnboundedReceiver<HookEvent>,
    CancellationToken,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let token = CancellationToken::new();
    let last_total_tokens = Arc::new(AtomicU64::new(0));
    let hook = CopilotPromptHook::new(tx, token.clone(), last_total_tokens);
    (hook, rx, token)
}

fn make_hook_with_shared_tokens() -> (
    CopilotPromptHook,
    mpsc::UnboundedReceiver<HookEvent>,
    Arc<AtomicU64>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let token = CancellationToken::new();
    let last_total_tokens = Arc::new(AtomicU64::new(0));
    let hook = CopilotPromptHook::new(tx, token, last_total_tokens.clone());
    (hook, rx, last_total_tokens)
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
            Some(rig::completion::request::Usage {
                input_tokens: 150,
                output_tokens: 75,
                total_tokens: 225,
                cached_input_tokens: 0,
                cache_creation_input_tokens: 0,
                tool_use_prompt_tokens: 0,
                reasoning_tokens: 0,
            })
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
        // First event is ToolStart (before permission check)
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, HookEvent::ToolStart { .. }));
        // Second event is AskPermission
        if let Some(HookEvent::AskPermission { responder, .. }) = rx.recv().await {
            responder.send(PermissionDecision::Allow).unwrap();
        }
    });

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "read_file", None, "id1", "{}").await;
    assert!(matches!(result, ToolCallHookAction::Continue));
}

#[tokio::test]
async fn on_tool_call_denied_returns_skip() {
    let (hook, mut rx) = make_hook();

    let handle = tokio::spawn(async move {
        // First event is ToolStart
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, HookEvent::ToolStart { .. }));
        // Second event is AskPermission
        if let Some(HookEvent::AskPermission { responder, .. }) = rx.recv().await {
            responder.send(PermissionDecision::Deny).unwrap();
        }
        // Third event is ToolEnd with success=false
        let end_event = rx.recv().await.unwrap();
        match end_event {
            HookEvent::ToolEnd { name, success, .. } => {
                assert_eq!(name, "write_file");
                assert!(!success);
            }
            _ => panic!("Expected ToolEnd event after deny"),
        }
    });

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "write_file", None, "id1", "{}").await;
    assert!(matches!(result, ToolCallHookAction::Skip { .. }));
    handle.await.unwrap();
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

    use rig::message::Message;
    use rig::one_or_many::OneOrMany;

    let msg = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "hi".to_string(),
            additional_params: None,
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
        // First event is ToolStart
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, HookEvent::ToolStart { .. }));
        // Second event is AskPermission
        if let Some(HookEvent::AskPermission {
            tool_call_id,
            responder,
            ..
        }) = rx.recv().await
        {
            assert_eq!(tool_call_id, Some("call_xyz789".to_string()));
            responder.send(PermissionDecision::Allow).unwrap();
        } else {
            panic!("Expected AskPermission event");
        }
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
        // First event is ToolStart
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, HookEvent::ToolStart { .. }));
        // Second event is AskPermission
        if let Some(HookEvent::AskPermission {
            tool_call_id,
            responder,
            ..
        }) = rx.recv().await
        {
            assert_eq!(tool_call_id, None);
            responder.send(PermissionDecision::Allow).unwrap();
        } else {
            panic!("Expected AskPermission event");
        }
    });

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "read_file", None, "internal_id", "{}").await;

    assert!(matches!(result, ToolCallHookAction::Continue));
    handle.await.unwrap();
}

// Tests for streaming usage reporting via on_stream_completion_response_finish

#[tokio::test]
async fn on_stream_completion_response_finish_emits_usage() {
    use rig::message::Message;
    use rig::one_or_many::OneOrMany;
    use test_support::{DummyModel, DummyStreamResponse};

    let (hook, mut rx) = make_hook();

    let prompt = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "test prompt".to_string(),
            additional_params: None,
        })),
    };

    let response = DummyStreamResponse;

    let result =
        PromptHook::<DummyModel>::on_stream_completion_response_finish(&hook, &prompt, &response)
            .await;
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
            assert_eq!(response_chars, 0);
            assert_eq!(tool_calls, 0);
            assert_eq!(input_tokens, 150);
            assert_eq!(output_tokens, 75);
            assert_eq!(total_tokens, 225);
        }
        _ => panic!("Expected LlmEnd event"),
    }
}

#[cfg(test)]
mod no_usage_support {
    use rig::completion::request::{
        CompletionError, CompletionModel, CompletionRequest, CompletionResponse,
    };
    use rig::streaming::StreamingCompletionResponse;
    use serde::{Deserialize, Serialize};

    #[derive(Clone)]
    pub struct DummyModelNoUsage;

    #[derive(Clone, Serialize, Deserialize)]
    pub struct DummyStreamResponseNoUsage;

    impl rig::completion::request::GetTokenUsage for DummyStreamResponseNoUsage {
        fn token_usage(&self) -> Option<rig::completion::request::Usage> {
            None
        }
    }

    impl CompletionModel for DummyModelNoUsage {
        type Response = serde_json::Value;
        type StreamingResponse = DummyStreamResponseNoUsage;
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

#[tokio::test]
async fn on_stream_completion_response_finish_no_usage_no_event() {
    use rig::agent::PromptHook;

    use rig::message::Message;
    use rig::one_or_many::OneOrMany;
    use tokio::sync::mpsc::error::TryRecvError;

    use no_usage_support::{DummyModelNoUsage, DummyStreamResponseNoUsage};

    let (hook, mut rx) = make_hook();

    let prompt = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "test prompt".to_string(),
            additional_params: None,
        })),
    };

    let response = DummyStreamResponseNoUsage;

    let result = PromptHook::<DummyModelNoUsage>::on_stream_completion_response_finish(
        &hook, &prompt, &response,
    )
    .await;
    assert!(matches!(result, HookAction::Continue));

    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn on_stream_completion_response_finish_stores_last_total_tokens() {
    use rig::message::Message;
    use rig::one_or_many::OneOrMany;
    use test_support::{DummyModel, DummyStreamResponse};

    let (hook, _rx, shared_tokens) = make_hook_with_shared_tokens();

    // Verify starts at 0
    assert_eq!(shared_tokens.load(Ordering::Relaxed), 0);

    let prompt = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "test prompt".to_string(),
            additional_params: None,
        })),
    };

    let response = DummyStreamResponse; // returns total_tokens = 225

    let result =
        PromptHook::<DummyModel>::on_stream_completion_response_finish(&hook, &prompt, &response)
            .await;
    assert!(matches!(result, HookAction::Continue));

    // Verify the shared AtomicU64 was updated with total_tokens from DummyStreamResponse
    assert_eq!(shared_tokens.load(Ordering::Relaxed), 225);
}
