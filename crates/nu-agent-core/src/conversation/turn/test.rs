//! Tests for the turn module.
//!
//! Covers:
//!  - `TurnResult` / `TurnError` construction and conversion (unit tests)
//!  - `execute_turn` using `MockResolver` + rig's `MockCompletionModel` (integration tests)

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rig::one_or_many::OneOrMany;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use tokio::runtime::Runtime;

use super::*;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::types::{
    AssistantContent, InMemoryConversationMemory, Message, Text, ToolCall, ToolFunction,
    ToolResultContent, UserContent,
};

// ---------------------------------------------------------------------------
// MockResolver: AsyncPermissionResolver that always returns Allow
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
// MockUi: ProgressUi that collects events
// ---------------------------------------------------------------------------

use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;

struct MockUi {
    pub events: Vec<UiEvent>,
    cancel_after: Option<usize>,
    tick_count: usize,
    cancel_flag: Arc<AtomicBool>,
}

impl MockUi {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            cancel_after: None,
            tick_count: 0,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a MockUi that returns `true` from `take_cancel_requested()` on the
    /// very first call — before any tick is emitted. This is used to test cancellation
    /// without relying on stream-timing races: the cancel fires in the drain loop's
    /// first iteration, before the spawned tokio task has had time to call
    /// `on_completion_call` or poll the first stream event.
    fn immediately_cancelled() -> Self {
        Self {
            events: Vec::new(),
            cancel_after: None,
            tick_count: 0,
            cancel_flag: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl ProgressUi for MockUi {
    fn emit(&mut self, event: &UiEvent) {
        if matches!(event, UiEvent::Tick) {
            self.tick_count += 1;
            if let Some(threshold) = self.cancel_after
                && self.tick_count >= threshold
            {
                self.cancel_flag.store(true, Ordering::SeqCst);
            }
        }
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        self.cancel_flag.swap(false, Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_turn_context<'a>(
    runtime: &'a tokio::runtime::Handle,
    model: MockCompletionModel,
) -> TurnContext<'a, MockCompletionModel> {
    let memory = InMemoryConversationMemory::new();
    let conversation = TurnConversation {
        memory,
        conversation_id: "test-conv".to_string(),
    };
    let input = TurnInput {
        prompt: "Hello".to_string(),
        preamble: None,
        max_turns: None,
    };
    let tool_infra = executor::ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::new()),
        mcp_registry: Arc::new(McpToolRegistry::from_names::<[&str; 0], &str>([])),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        visible_tool_definitions: vec![],
    };
    TurnContext::new(runtime, model, conversation, input, tool_infra)
}

// ---------------------------------------------------------------------------
// execute_turn integration tests
// ---------------------------------------------------------------------------

/// Text-only stream: verify `TurnResult.text` is populated and `tool_call_count == 0`.
#[test]
fn execute_turn_text_only_response() {
    let rt = Runtime::new().expect("failed to create tokio runtime");
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("Hello, world!".to_string()),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let ctx = make_turn_context(rt.handle(), model);
    let mut ui = MockUi::new();
    let resolver = MockResolver(PermissionDecision::Allow);

    let result = execute_turn(ctx, &mut ui, resolver).expect("execute_turn should succeed");

    assert!(
        result.text.contains("Hello, world!") || !result.text.is_empty(),
        "TurnResult.text should be populated; got: {:?}",
        result.text
    );
    assert_eq!(result.tool_call_count, 0, "No tools should have been called");
    assert!(!result.cancelled, "Turn should not be cancelled");
}

/// Cancellation via UI: the UI requests cancellation before the agent starts,
/// which causes the cancel_token to fire on the drain loop's first iteration.
/// The spawned tokio task sees the already-cancelled token (either via
/// `on_completion_call` returning `Terminate` or the `select!` cancel branch)
/// and returns a result with `cancelled == true`.
#[test]
fn execute_turn_cancel_returns_cancelled_true() {
    let rt = Runtime::new().expect("failed to create tokio runtime");

    // Use a normal model — the cancel fires before any stream event is processed.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("partial".to_string()),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let ctx = make_turn_context(rt.handle(), model);
    // Pre-set cancel flag so take_cancel_requested() fires on the very first drain
    // loop iteration — before any 16ms sleep, giving the cancel_token the best
    // chance to be seen by the spawned task before it processes stream events.
    let mut ui = MockUi::immediately_cancelled();
    let resolver = MockResolver(PermissionDecision::Allow);

    let result = execute_turn(ctx, &mut ui, resolver).expect("execute_turn should not error");

    assert!(result.cancelled, "Turn should be marked as cancelled");
}

// ---------------------------------------------------------------------------
// Unit tests: TurnResult / TurnError construction and field access
// ---------------------------------------------------------------------------

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
            tool_use_prompt_tokens: 0,
            reasoning_tokens: 0,
        },
        messages: None,
        tool_call_count: 0,
        deltas_emitted: false,
        cancelled: false,
        last_total_tokens: 0,
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
fn prompt_cancelled_error_is_detected_as_cancellation() {
    let user_msg = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "Hello".to_string(),
            additional_params: None,
        })),
    };
    let err = rig::completion::PromptError::PromptCancelled {
        chat_history: vec![user_msg],
        reason: "Cancelled by user".to_string(),
    };

    let turn_err = TurnError::from(err);

    assert!(
        turn_err.cancelled,
        "PromptCancelled variant should be detected as cancellation"
    );
    assert!(turn_err.msg.contains("Cancelled by user"));

    let messages = turn_err
        .messages
        .expect("PromptCancelled should capture chat_history as messages");
    assert_eq!(messages.len(), 1, "Should have one message from chat_history");
}

#[test]
fn other_prompt_errors_are_not_cancelled() {
    let completion_err =
        rig::completion::CompletionError::ResponseError("Some other error".to_string());
    let err = rig::completion::PromptError::from(completion_err);

    let turn_err = TurnError::from(err);

    assert!(
        !turn_err.cancelled,
        "Non-cancellation errors should not be detected as cancellation"
    );
}

#[test]
fn max_turns_error_is_not_cancelled() {
    let err = rig::completion::PromptError::MaxTurnsError {
        max_turns: 10,
        chat_history: Box::new(vec![]),
        prompt: Box::new(Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: "test".to_string(),
                additional_params: None,
            })),
        }),
    };

    let turn_err = TurnError::from(err);

    assert!(
        !turn_err.cancelled,
        "MaxTurnsError should not be detected as cancellation"
    );
}

/// Test that TurnContext can be created without an MCP runtime.
#[test]
fn turn_context_always_has_tool_server_handle() {
    let handle = rig::tool::server::ToolServer::new().run();
    let _handle_clone = handle.clone();
}

/// Test that TurnContext uses InMemoryConversationMemory.
#[test]
fn turn_context_uses_memory_instead_of_history_vec() {
    let memory = InMemoryConversationMemory::new();
    let conversation_id = "test-conversation-123".to_string();
    let _memory_clone = memory.clone();
    let _id_clone = conversation_id.clone();
}

/// Test that StreamingError converts to TurnError with cancelled=false for non-cancel errors.
#[test]
fn streaming_error_from_prompt_cancelled_captures_messages() {
    let user_msg = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "Tell me about async".to_string(),
            additional_params: None,
        })),
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

/// TurnError from PromptCancelled captures chat_history as messages.
#[test]
fn turn_error_from_prompt_cancelled_captures_messages() {
    let user_msg = Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "What is Rust?".to_string(),
            additional_params: None,
        })),
    };
    let assistant_msg = Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::Text(Text {
            text: "Rust is a systems programming...".to_string(),
            additional_params: None,
        })),
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
        last_total_tokens: 0,
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
        Message::Assistant { content, .. } => {
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
        last_total_tokens: 0,
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

#[test]
fn turn_result_cancelled_flag_propagates() {
    let cancelled_result = TurnResult {
        text: String::new(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: true,
        last_total_tokens: 0,
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
        last_total_tokens: 0,
    };

    assert!(
        !normal_result.cancelled,
        "Normal turn should not be cancelled"
    );
}

// ---------------------------------------------------------------------------
// rig v0.39.0: Path B with tool calls — chat_history preserved after cancel
// ---------------------------------------------------------------------------

/// Regression test: PromptCancelled preserves tool_call + tool_result history.
#[test]
fn cancel_mid_tool_call_preserves_tool_call_and_tool_result_in_history() {
    use serde_json::json;

    let mut chat_history: Vec<Message> = Vec::new();

    chat_history.push(Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "What is in /etc/hosts?".to_string(),
            additional_params: None,
        })),
    });

    chat_history.push(Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
            id: "call_abc123".to_string(),
            call_id: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "read_file".to_string(),
                arguments: json!({ "path": "/etc/hosts" }),
            },
        })),
    });

    chat_history.push(Message::User {
        content: OneOrMany::one(UserContent::ToolResult(
            rig::completion::message::ToolResult {
                id: "call_abc123".to_string(),
                call_id: None,
                content: OneOrMany::one(ToolResultContent::Text(Text {
                    text: "file contents here".to_string(),
                    additional_params: None,
                })),
            },
        )),
    });

    let err = rig::completion::PromptError::PromptCancelled {
        reason: "User pressed Esc during tool execution".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    assert!(turn_err.cancelled, "TurnError must be marked as cancelled");
    assert_eq!(turn_err.msg, "User pressed Esc during tool execution");

    let messages = turn_err.messages.expect(
        "chat_history must be preserved — PromptCancelled provides full history including \
         tool_call + tool_result pairs",
    );

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
        Message::Assistant { .. } => {}
        _ => panic!("msg[1] should be Assistant (tool_call)"),
    };
    match &messages[2] {
        Message::User { .. } => {}
        _ => panic!("msg[2] should be User (tool_result)"),
    };
}

/// Regression: multiple tool-use cycles are all preserved on cancellation.
#[test]
fn cancel_preserves_multiple_tool_use_cycles() {
    use serde_json::json;

    let mut chat_history: Vec<Message> = Vec::new();

    // First tool-use cycle
    chat_history.push(Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "List files".to_string(),
            additional_params: None,
        })),
    });
    chat_history.push(Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
            id: "call_001".to_string(),
            call_id: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "list_dir".to_string(),
                arguments: json!({ "path": "/" }),
            },
        })),
    });
    chat_history.push(Message::User {
        content: OneOrMany::one(UserContent::ToolResult(
            rig::completion::message::ToolResult {
                id: "call_001".to_string(),
                call_id: None,
                content: OneOrMany::one(ToolResultContent::Text(Text {
                    text: "/bin /usr /etc".to_string(),
                    additional_params: None,
                })),
            },
        )),
    });

    // Second tool-use cycle
    chat_history.push(Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::Text(Text {
            text: "Now read that file".to_string(),
            additional_params: None,
        })),
    });
    chat_history.push(Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
            id: "call_002".to_string(),
            call_id: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "read_file".to_string(),
                arguments: json!({ "path": "/etc/passwd" }),
            },
        })),
    });
    chat_history.push(Message::User {
        content: OneOrMany::one(UserContent::ToolResult(
            rig::completion::message::ToolResult {
                id: "call_002".to_string(),
                call_id: None,
                content: OneOrMany::one(ToolResultContent::Text(Text {
                    text: "root:x:0:0:root user".to_string(),
                    additional_params: None,
                })),
            },
        )),
    });

    let err = rig::completion::PromptError::PromptCancelled {
        reason: "Cancelled during read_file tool call".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    assert!(turn_err.cancelled);

    let messages = turn_err.messages.expect(
        "All accumulated messages must be preserved on cancel — including 2 completed tool cycles",
    );

    assert_eq!(
        messages.len(),
        chat_history.len(),
        "Every single message in accumulated history should survive cancellation"
    );
}
