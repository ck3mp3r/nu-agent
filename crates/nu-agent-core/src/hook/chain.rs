//! `HookChain<P>` — composable hook implementation built from named concern structs.
//!
//! Each concern (cancellation, sub-turn cap, doom loop, circuit breaker, history
//! snapshot) lives in its own module and has its own unit tests. `HookChain` owns
//! them as named fields and delegates to them in an explicit, readable order inside
//! the `PromptHook<M>` impl.
//!
//! ## Adding a new concern
//!
//! 1. Create `new_concern.rs` + `new_concern_test.rs` under `hook/`.
//! 2. Add a named field on `HookChain`.
//! 3. Call the concern's method inside whichever hook method needs it.
//! 4. Nothing else changes.

use std::sync::Arc;

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
use crate::tools::handler::{McpToolRegistry, ToolSource, builtin_kinds::BuiltinKind};

use super::cancel::CancelChecker;
use super::circuit_breaker_guard::CircuitBreakerGuard;
use super::doom_loop::DoomLoopDetector;
use super::history_snapshot::HistorySnapshot;
use super::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use super::subturn_cap::SubTurnCap;

fn resolve_tool_source(
    name: &str,
    closures: &ClosureRegistry,
    mcp: &McpToolRegistry,
) -> ToolSource {
    if closures.get(name).is_some() {
        ToolSource::Closure
    } else if let Ok(kind) = name.parse::<BuiltinKind>() {
        if kind.is_fs() {
            ToolSource::BuiltinFs
        } else {
            ToolSource::Builtin
        }
    } else if mcp.contains(name) {
        ToolSource::Mcp
    } else {
        ToolSource::Unknown
    }
}

/// Default cap on tool calls per sub-turn when not configured.
const DEFAULT_MAX_TOOL_CALLS_PER_SUBTURN: usize = 10;

/// Composable hook that delegates to named concern structs in explicit order.
///
/// See module-level docs for the extension pattern.
#[derive(Clone)]
pub struct HookChain<P: AsyncPermissionResolver> {
    cancel: CancelChecker,
    subturn: SubTurnCap,
    doom: DoomLoopDetector,
    circuit: CircuitBreakerGuard,
    history: HistorySnapshot,
    permission: P,
    ui_tx: mpsc::UnboundedSender<UiEvent>,
    closure_registry: Arc<ClosureRegistry>,
    mcp_registry: Arc<McpToolRegistry>,
}

impl<P: AsyncPermissionResolver> HookChain<P> {
    pub fn new(
        cancel_token: CancellationToken,
        ui_tx: mpsc::UnboundedSender<UiEvent>,
        permission_resolver: P,
        closure_registry: Arc<ClosureRegistry>,
        mcp_registry: Arc<McpToolRegistry>,
        max_tool_calls_per_subturn: Option<usize>,
        hook_state: super::agent_hook::HookState,
    ) -> Self {
        Self {
            cancel: CancelChecker {
                token: cancel_token,
            },
            subturn: SubTurnCap::new(
                max_tool_calls_per_subturn.unwrap_or(DEFAULT_MAX_TOOL_CALLS_PER_SUBTURN),
            ),
            doom: DoomLoopDetector {
                state: hook_state.doom_state,
            },
            circuit: CircuitBreakerGuard {
                breaker: hook_state.circuit_breaker,
            },
            history: HistorySnapshot::new(),
            permission: permission_resolver,
            ui_tx,
            closure_registry,
            mcp_registry,
        }
    }

    /// Return a clone of the `Arc` that holds the most recent history snapshot.
    ///
    /// Callers clone this Arc **before** passing the hook into the agent builder
    /// (which consumes `self`), then read it back after a `CompletionError`.
    pub fn last_known_history(&self) -> std::sync::Arc<std::sync::Mutex<Vec<Message>>> {
        self.history.arc()
    }
}

impl<M, P> PromptHook<M> for HookChain<P>
where
    M: CompletionModel,
    P: AsyncPermissionResolver,
{
    async fn on_completion_call(&self, prompt: &Message, history: &[Message]) -> HookAction {
        // 1. History snapshot — store history + prompt before the LLM call
        self.history.update(history, prompt);

        // 2. Reset sub-turn cap — new LLM request, fresh counter
        self.subturn.reset();

        // Trace-level payload logging for debugging LLM request contents.
        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "on_completion_call: history_len={} prompt={}",
                history.len(),
                serde_json::to_string(prompt).unwrap_or_else(|_| "<serialize error>".into()),
            );
            for (i, msg) in history.iter().enumerate() {
                log::trace!(
                    "  history[{}]: {}",
                    i,
                    serde_json::to_string(msg).unwrap_or_else(|_| "<serialize error>".into()),
                );
            }
        } else if log::log_enabled!(log::Level::Debug) {
            log::trace!("on_completion_call: history_len={}", history.len());
        }

        if let Some(action) = self.cancel.check_hook() {
            return action;
        }

        // 3. Send LlmStart
        let _ = self.ui_tx.send(UiEvent::LlmStart);
        HookAction::Continue
    }

    async fn on_text_delta(&self, _delta: &str, aggregated: &str) -> HookAction {
        if let Some(action) = self.cancel.check_hook() {
            return action;
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
        log::trace!("on_tool_call: tool={tool_name}");

        // 1. Check cancellation
        if let Some(action) = self.cancel.check_tool_call() {
            return action;
        }

        // 2. Check per-sub-turn tool call cap
        if let Some(action) = self.subturn.check_and_increment(tool_name) {
            return action;
        }

        // 3. Check doom loop
        if let Some(action) = self.doom.check_and_record(tool_name, args, &self.ui_tx) {
            return action;
        }

        // 4. Check circuit breaker (MCP server disabled)
        if let Some(action) = self
            .circuit
            .check_server_enabled(tool_name, &self.mcp_registry)
        {
            return action;
        }

        // 5. Resolve tool source
        let source = resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry)
            .as_str()
            .to_string();

        // 6. Announce tool call
        let _ = self.ui_tx.send(UiEvent::ToolStart {
            name: tool_name.to_string(),
            source: source.clone(),
            arguments: args.to_string(),
        });

        // 7. Ask permission
        let decision = self
            .permission
            .resolve(tool_name, args, tool_call_id, Some(self.ui_tx.clone()))
            .await;

        // 8. Act on decision
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
        log::trace!(
            "on_tool_result: tool={tool_name} success={} result_len={}",
            !super::agent_hook::is_tool_failure(result),
            result.len(),
        );
        if log::log_enabled!(log::Level::Trace) {
            let preview = if result.len() > 2000 {
                format!("{}...<truncated {} bytes>", &result[..2000], result.len())
            } else {
                result.to_string()
            };
            log::trace!("  result_body: {preview}");
        }

        // 1. Parse result JSON and extract display
        let display = serde_json::from_str::<serde_json::Value>(result)
            .ok()
            .and_then(|json| crate::tools::handler::build_direct_tool_display(tool_name, &json));

        // 2. Resolve source
        let source = resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry)
            .as_str()
            .to_string();

        // 3. Emit ToolEnd
        let success = !super::agent_hook::is_tool_failure(result);

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

        // 4. Circuit breaker — track transport failures per server
        self.circuit
            .record_result(tool_name, result, success, &self.mcp_registry, &self.ui_tx);

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
        log::warn!("Invalid tool call: tool={}", context.tool_name);
        let feedback = format!(
            "Tool '{}' is not available. Available tools: [{}]",
            context.tool_name,
            context.available_tools.join(", ")
        );
        let ui_tx = self.ui_tx.clone();
        async move {
            let _ = ui_tx.send(UiEvent::Warning {
                message: feedback.clone(),
            });
            InvalidToolCallHookAction::Retry { feedback }
        }
    }
}
