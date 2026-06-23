use super::*;
use crate::protocol::event::UiEvent;
use crate::types::{Text, UserContent};
use rig::agent::{InvalidToolCallContext, InvalidToolCallHookAction, PromptHook};
use rig::one_or_many::OneOrMany;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// DummyModel for PromptHook<M> tests
// ---------------------------------------------------------------------------

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
        fn token_usage(&self) -> rig::completion::request::Usage {
            rig::completion::request::Usage {
                input_tokens: 100,
                output_tokens: 50,
                total_tokens: 150,
                cached_input_tokens: 0,
                cache_creation_input_tokens: 0,
                tool_use_prompt_tokens: 0,
                reasoning_tokens: 0,
            }
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

use test_support::{DummyModel, DummyStreamResponse};

// ---------------------------------------------------------------------------
// MockResolver
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct MockResolver(PermissionDecision);

impl AsyncPermissionResolver for MockResolver {
    fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let decision = self.0;
        async move { decision }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_hook_with_resolver(
    resolver: MockResolver,
) -> (AgentHook<MockResolver>, mpsc::UnboundedReceiver<UiEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let token = CancellationToken::new();
    let closure_registry = Arc::new(crate::tools::closure::ClosureRegistry::new());
    let mcp_registry = Arc::new(crate::tools::handler::McpToolRegistry::from_names(
        std::iter::empty::<String>(),
    ));
    let hook = AgentHook::new(token, tx, resolver, closure_registry, mcp_registry);
    (hook, rx)
}

fn make_hook_with_token(
    resolver: MockResolver,
) -> (
    AgentHook<MockResolver>,
    mpsc::UnboundedReceiver<UiEvent>,
    CancellationToken,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let token = CancellationToken::new();
    let closure_registry = Arc::new(crate::tools::closure::ClosureRegistry::new());
    let mcp_registry = Arc::new(crate::tools::handler::McpToolRegistry::from_names(
        std::iter::empty::<String>(),
    ));
    let hook = AgentHook::new(token.clone(), tx, resolver, closure_registry, mcp_registry);
    (hook, rx, token)
}

fn dummy_message() -> rig::message::Message {
    rig::message::Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "hi".to_string(),
            additional_params: None,
        })),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn on_tool_call_allow_returns_continue() {
    let (hook, _rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Allow));

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "read_file", None, "id1", "{}").await;
    assert!(matches!(result, ToolCallHookAction::Continue));
}

#[tokio::test]
async fn on_tool_call_deny_returns_skip_and_emits_tool_end() {
    let (hook, mut rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Deny));

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "write_file", None, "id1", "{}").await;
    assert!(matches!(result, ToolCallHookAction::Skip { .. }));

    // ToolStart should be emitted first
    let start_event = rx.try_recv().expect("expected ToolStart event");
    assert!(
        matches!(start_event, UiEvent::ToolStart { .. }),
        "expected ToolStart, got {start_event:?}"
    );

    // ToolEnd should be emitted with success=false
    let end_event = rx.try_recv().expect("expected ToolEnd event");
    match end_event {
        UiEvent::ToolEnd {
            name,
            success,
            message,
            ..
        } => {
            assert_eq!(name, "write_file");
            assert!(!success, "expected success=false on deny");
            assert_eq!(message.as_deref(), Some("Permission denied"));
        }
        other => panic!("expected ToolEnd event, got {other:?}"),
    }
}

#[tokio::test]
async fn on_tool_call_cancelled_returns_terminate() {
    let (hook, _rx, token) = make_hook_with_token(MockResolver(PermissionDecision::Allow));
    token.cancel();

    let result =
        PromptHook::<DummyModel>::on_tool_call(&hook, "read_file", None, "id1", "{}").await;
    assert!(matches!(result, ToolCallHookAction::Terminate { .. }));
}

#[tokio::test]
async fn on_text_delta_emits_assistant_message() {
    let (hook, mut rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Allow));

    let result = PromptHook::<DummyModel>::on_text_delta(&hook, "world", "hello world").await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().expect("expected AssistantMessage event");
    match event {
        UiEvent::AssistantMessage { text } => {
            assert_eq!(text, "hello world");
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[tokio::test]
async fn on_tool_result_emits_tool_end_success() {
    let (hook, mut rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Allow));

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

    let event = rx.try_recv().expect("expected ToolEnd event");
    match event {
        UiEvent::ToolEnd {
            name,
            success,
            result,
            ..
        } => {
            assert_eq!(name, "read_file");
            assert!(success, "expected success=true");
            assert_eq!(result, "file contents");
        }
        other => panic!("expected ToolEnd, got {other:?}"),
    }
}

#[tokio::test]
async fn on_completion_call_emits_llm_start() {
    let (hook, mut rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Allow));

    let msg = dummy_message();
    let result = PromptHook::<DummyModel>::on_completion_call(&hook, &msg, &[]).await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().expect("expected LlmStart event");
    assert!(
        matches!(event, UiEvent::LlmStart),
        "expected LlmStart, got {event:?}"
    );
}

#[tokio::test]
async fn on_stream_completion_response_finish_emits_llm_end() {
    let (hook, mut rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Allow));

    let msg = dummy_message();
    let response = DummyStreamResponse;

    let result =
        PromptHook::<DummyModel>::on_stream_completion_response_finish(&hook, &msg, &response)
            .await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().expect("expected LlmEnd event");
    match event {
        UiEvent::LlmEnd {
            input_tokens,
            output_tokens,
            total_tokens,
            ..
        } => {
            assert_eq!(input_tokens, 100);
            assert_eq!(output_tokens, 50);
            assert_eq!(total_tokens, 150);
        }
        other => panic!("expected LlmEnd, got {other:?}"),
    }
}

#[tokio::test]
async fn on_invalid_tool_call_emits_warning_and_returns_skip() {
    let (hook, mut ui_rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Allow));

    let context = InvalidToolCallContext {
        tool_name: "nonexistent_tool".to_string(),
        tool_call_id: None,
        internal_call_id: None,
        args: None,
        available_tools: vec!["nu__run".to_string(), "nu__fs_read".to_string()],
        allowed_tools: vec![],
        tool_choice: None,
        chat_history: vec![],
        is_streaming: true,
    };

    let action = PromptHook::<DummyModel>::on_invalid_tool_call(&hook, &context).await;

    match action {
        InvalidToolCallHookAction::Skip { reason } => {
            assert!(reason.contains("nonexistent_tool"));
            assert!(reason.contains("nu__run"));
        }
        other => panic!("expected Skip, got {:?}", other),
    }

    let event = ui_rx.try_recv().expect("expected a UiEvent");
    match event {
        UiEvent::Warning { .. } => {}
        other => panic!("expected Warning, got {:?}", other),
    }
}

#[tokio::test]
async fn on_tool_result_emits_success_false_for_toolset_error() {
    let (hook, mut rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Allow));

    let result = PromptHook::<DummyModel>::on_tool_result(
        &hook,
        "some_tool",
        None,
        "id1",
        "{}",
        "Toolset error: ToolCallError: connection refused",
    )
    .await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().expect("expected ToolEnd event");
    match event {
        UiEvent::ToolEnd { success, .. } => {
            assert!(!success, "expected success=false for Toolset error prefix");
        }
        other => panic!("expected ToolEnd, got {other:?}"),
    }
}

#[tokio::test]
async fn on_tool_result_emits_success_true_for_normal_result() {
    let (hook, mut rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Allow));

    let result = PromptHook::<DummyModel>::on_tool_result(
        &hook,
        "some_tool",
        None,
        "id1",
        "{}",
        "{\"output\": \"hello\"}",
    )
    .await;
    assert!(matches!(result, HookAction::Continue));

    let event = rx.try_recv().expect("expected ToolEnd event");
    match event {
        UiEvent::ToolEnd { success, .. } => {
            assert!(success, "expected success=true for normal result");
        }
        other => panic!("expected ToolEnd, got {other:?}"),
    }
}

#[tokio::test]
async fn doom_loop_returns_skip_not_terminate() {
    let (hook, _rx) = make_hook_with_resolver(MockResolver(PermissionDecision::Allow));

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
                matches!(result, ToolCallHookAction::Skip { .. }),
                "iteration {i} should be Skip (not Terminate)"
            );
        }
    }
}
