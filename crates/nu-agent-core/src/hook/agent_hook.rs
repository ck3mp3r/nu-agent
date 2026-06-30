//! AgentHook — generic PromptHook implementation that emits UiEvents directly.
//!
//! Unlike `CopilotPromptHook`, `AgentHook<P>` owns an `AsyncPermissionResolver`
//! and writes `UiEvent` values straight to the UI channel, with no intermediate
//! `HookEvent` / driver layer.

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use rig::agent::{
    HookAction, InvalidToolCallContext, InvalidToolCallHookAction, PromptHook, ToolCallHookAction,
};
use rig::completion::GetTokenUsage;
use rig::completion::request::CompletionModel;
use rig::message::Message;

use crate::protocol::event::UiEvent;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

use super::permission_bridge::resolve_tool_source;
use super::permission_resolver::{AsyncPermissionResolver, PermissionDecision};

const DOOM_LOOP_THRESHOLD: usize = 5;

/// Returns `true` if a tool result string represents a failure injected by any
/// code path in the agent hook.
///
/// The following strings are failure indicators:
/// - `"Toolset error: "` — rig toolset execution errors
/// - `"Permission denied"` — permission denial from `on_tool_call`
/// - `"Doom loop detected: "` — doom loop guard in `on_tool_call`
/// - `"Tool '"` — invalid/unavailable tool skip from `on_invalid_tool_call`
/// - `"Tool call limit exceeded"` — per-sub-turn cap from `on_tool_call`
pub(crate) fn is_tool_failure(result_text: &str) -> bool {
    result_text.starts_with("Toolset error: ")
        || result_text == "Permission denied"
        || result_text.starts_with("Doom loop detected: ")
        || result_text.starts_with("Tool '")
        || result_text.starts_with("Tool call limit exceeded")
}

/// Tracks recent tool call signatures for doom loop detection.
#[derive(Debug, Clone, Default)]
struct DoomLoopState {
    recent_signatures: Vec<(String, String)>, // (tool_name, arguments)
}

impl DoomLoopState {
    /// Returns `Some(tool_name)` if a doom loop is detected.
    fn check_and_record(&mut self, name: &str, args: &str) -> Option<String> {
        self.recent_signatures
            .push((name.to_string(), args.to_string()));

        if self.recent_signatures.len() < DOOM_LOOP_THRESHOLD {
            return None;
        }

        let last_n = &self.recent_signatures[self.recent_signatures.len() - DOOM_LOOP_THRESHOLD..];
        let first = &last_n[0];
        if last_n.iter().all(|sig| sig == first) {
            Some(name.to_string())
        } else {
            None
        }
    }
}

/// Default cap on tool calls per sub-turn when not configured.
const DEFAULT_MAX_TOOL_CALLS_PER_SUBTURN: usize = 10;

/// Generic `PromptHook` implementation parameterized by an async permission resolver.
///
/// Emits `UiEvent` values directly via `ui_tx`, bypassing the `HookEvent` / `HookDriver`
/// bridge used by `CopilotPromptHook`.
#[derive(Clone)]
pub struct AgentHook<P: AsyncPermissionResolver> {
    cancel_token: CancellationToken,
    ui_tx: mpsc::UnboundedSender<UiEvent>,
    permission_resolver: P,
    doom_state: Arc<Mutex<DoomLoopState>>,
    closure_registry: Arc<ClosureRegistry>,
    mcp_registry: Arc<McpToolRegistry>,
    /// Snapshot of the full conversation history captured just before each LLM HTTP call.
    ///
    /// Updated by `on_completion_call` (which fires before every `stream()` call).
    /// If a `CompletionError` occurs, the caller can read this Arc to recover the
    /// history that was live at the time of the last LLM call attempt, rather than
    /// losing all completed sub-turns.
    last_known_history: Arc<Mutex<Vec<Message>>>,
    /// Maximum tool calls allowed in a single sub-turn (LLM response).
    /// 0 means unlimited.
    max_tool_calls_per_subturn: usize,
    /// Counter of tool calls executed in the current sub-turn, reset on each
    /// `on_completion_call` (which fires before each new LLM request).
    tool_calls_this_subturn: Arc<Mutex<usize>>,
}

impl<P: AsyncPermissionResolver> AgentHook<P> {
    pub fn new(
        cancel_token: CancellationToken,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
        permission_resolver: P,
        closure_registry: Arc<ClosureRegistry>,
        mcp_registry: Arc<McpToolRegistry>,
        max_tool_calls_per_subturn: Option<usize>,
    ) -> Self {
        Self {
            cancel_token,
            ui_tx,
            permission_resolver,
            doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
            closure_registry,
            mcp_registry,
            last_known_history: Arc::new(Mutex::new(Vec::new())),
            max_tool_calls_per_subturn: max_tool_calls_per_subturn
                .unwrap_or(DEFAULT_MAX_TOOL_CALLS_PER_SUBTURN),
            tool_calls_this_subturn: Arc::new(Mutex::new(0)),
        }
    }

    /// Return a clone of the `Arc` that holds the most recent history snapshot.
    ///
    /// Callers clone this Arc **before** passing the hook into the agent builder
    /// (which consumes `self`), then read it back after a `CompletionError` to
    /// recover whatever history was captured by the last `on_completion_call`.
    pub fn last_known_history(&self) -> Arc<Mutex<Vec<Message>>> {
        Arc::clone(&self.last_known_history)
    }
}

impl<M, P> PromptHook<M> for AgentHook<P>
where
    M: CompletionModel,
    P: AsyncPermissionResolver,
{
    async fn on_completion_call(&self, prompt: &Message, history: &[Message]) -> HookAction {
        let mut snapshot = history.to_vec();
        snapshot.push(prompt.clone());
        *self.last_known_history.lock().unwrap() = snapshot;
        // Reset the per-sub-turn tool call counter — a new LLM request is about
        // to fire, so any tool calls that follow belong to a fresh sub-turn.
        *self.tool_calls_this_subturn.lock().unwrap() = 0;
        if self.cancel_token.is_cancelled() {
            return HookAction::Terminate {
                reason: "Cancelled by user".into(),
            };
        }
        let _ = self.ui_tx.send(UiEvent::LlmStart);
        HookAction::Continue
    }

    async fn on_text_delta(&self, _delta: &str, aggregated: &str) -> HookAction {
        if self.cancel_token.is_cancelled() {
            return HookAction::Terminate {
                reason: "Cancelled by user".into(),
            };
        }
        let _ = self.ui_tx.send(UiEvent::AssistantMessage {
            text: aggregated.to_string(),
        });
        HookAction::Continue
    }

    async fn on_tool_call(
        &self,
        tool_name: &str,
        tool_call_id: Option<String>,
        _internal_call_id: &str,
        args: &str,
    ) -> ToolCallHookAction {
        // 1. Check cancellation
        if self.cancel_token.is_cancelled() {
            return ToolCallHookAction::Terminate {
                reason: "Cancelled by user".into(),
            };
        }

        // 2. Check per-sub-turn tool call cap
        if self.max_tool_calls_per_subturn > 0 {
            let mut count = self.tool_calls_this_subturn.lock().unwrap();
            if *count >= self.max_tool_calls_per_subturn {
                return ToolCallHookAction::Skip {
                    reason: "Tool call limit exceeded for this sub-turn. Remaining calls skipped."
                        .to_string(),
                };
            }
            *count += 1;
        }

        // 3. Check doom loop
        {
            let mut state = self.doom_state.lock().unwrap();
            if let Some(tool) = state.check_and_record(tool_name, args) {
                let message = format!(
                    "Doom loop detected: '{}' called {} times with identical arguments",
                    tool, DOOM_LOOP_THRESHOLD
                );
                let _ = self.ui_tx.send(UiEvent::Warning {
                    message: message.clone(),
                });
                return ToolCallHookAction::Skip { reason: message };
            }
        }

        // 4. Resolve tool source
        let source = resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry)
            .as_str()
            .to_string();

        // 5. Announce tool call
        let _ = self.ui_tx.send(UiEvent::ToolStart {
            name: tool_name.to_string(),
            source: source.clone(),
            arguments: args.to_string(),
        });

        // 6. Ask permission
        let decision = self
            .permission_resolver
            .resolve(tool_name, args, tool_call_id, Some(self.ui_tx.clone()))
            .await;

        // 7. Act on decision
        match decision {
            PermissionDecision::Allow => ToolCallHookAction::Continue,
            PermissionDecision::Deny => {
                let _ = self.ui_tx.send(UiEvent::ToolEnd {
                    name: tool_name.to_string(),
                    source,
                    arguments: args.to_string(),
                    success: false,
                    result: String::new(),
                    display: None,
                    error_kind: None,
                    message: Some("Permission denied".to_string()),
                });
                ToolCallHookAction::Skip {
                    reason: "Permission denied".to_string(),
                }
            }
        }
    }

    async fn on_tool_result(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        args: &str,
        result: &str,
    ) -> HookAction {
        // 1. Parse result JSON and extract display
        let display = serde_json::from_str::<serde_json::Value>(result)
            .ok()
            .and_then(|json| crate::tools::handler::build_direct_tool_display(tool_name, &json));

        // 2. Resolve source
        let source = resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry)
            .as_str()
            .to_string();

        // 3. Emit ToolEnd
        // Errors from rig's tool execution chain always start with "Toolset error: ".
        // Successful results never do.
        let success = !is_tool_failure(result);

        let _ = self.ui_tx.send(UiEvent::ToolEnd {
            name: tool_name.to_string(),
            source,
            arguments: args.to_string(),
            success,
            result: result.to_string(),
            display,
            error_kind: None,
            message: None,
        });

        HookAction::Continue
    }

    async fn on_stream_completion_response_finish(
        &self,
        _prompt: &Message,
        response: &M::StreamingResponse,
    ) -> HookAction {
        let usage = response.token_usage();
        if usage.total_tokens > 0 {
            let _ = self.ui_tx.send(UiEvent::LlmEnd {
                response_chars: 0,
                tool_calls: 0,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
            });
        }
        HookAction::Continue
    }

    // on_tool_call_delta: use default impl

    fn on_invalid_tool_call(
        &self,
        context: &InvalidToolCallContext,
    ) -> impl std::future::Future<Output = InvalidToolCallHookAction> + Send {
        let reason = format!(
            "Tool '{}' is not available. Available tools: [{}]",
            context.tool_name,
            context.available_tools.join(", ")
        );
        let ui_tx = self.ui_tx.clone();
        async move {
            let _ = ui_tx.send(UiEvent::Warning {
                message: reason.clone(),
            });
            InvalidToolCallHookAction::Skip { reason }
        }
    }
}

#[cfg(test)]
#[path = "agent_hook_test.rs"]
mod agent_hook_test;
