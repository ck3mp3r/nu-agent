//! `HookChain<P>` — composable hook implementation built from named concern structs.
//!
//! Each concern (cancellation, sub-turn cap, doom loop, circuit breaker, history
//! snapshot) lives in its own module and has its own unit tests. `HookChain` owns
//! them as named fields and delegates to them in an explicit, readable order inside
//! the `AgentHook` impl.
//!
//! ## Adding a new concern
//!
//! 1. Create `new_concern.rs` + `new_concern_test.rs` under `hook/`.
//! 2. Add a named field on `HookChain`.
//! 3. Call the concern's method inside whichever event method needs it.
//! 4. Nothing else changes.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use rig::agent::{
    AgentHook, CompletionCallAction, CompletionCallEvent, HookContext, InvalidToolCallAction,
    InvalidToolCallContext, ObservationAction, StreamResponseFinish, TextDelta, ToolCall,
    ToolCallAction, ToolResultAction, ToolResultEvent,
};
use rig::core::wasm_compat::WasmCompatSend;
use rig::message::Message;

use crate::config::defaults;
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
        if kind.is_privileged() {
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
                max_tool_calls_per_subturn.unwrap_or(defaults::MAX_TOOL_CALLS_PER_SUBTURN),
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

impl<P: AsyncPermissionResolver> AgentHook for HookChain<P> {
    fn on_completion_call(
        &self,
        _ctx: &HookContext,
        event: CompletionCallEvent<'_>,
    ) -> impl std::future::Future<Output = CompletionCallAction> + WasmCompatSend {
        // 1. History snapshot — store history + prompt before the LLM call
        self.history.update(event.history, event.prompt);

        // 2. Reset sub-turn cap — new LLM request, fresh counter
        self.subturn.reset();

        // Trace-level payload logging for debugging LLM request contents.
        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "on_completion_call: history_len={} prompt={}",
                event.history.len(),
                serde_json::to_string(event.prompt).unwrap_or_else(|_| "<serialize error>".into()),
            );
            for (i, msg) in event.history.iter().enumerate() {
                log::trace!(
                    "  history[{}]: {}",
                    i,
                    serde_json::to_string(msg).unwrap_or_else(|_| "<serialize error>".into()),
                );
            }
        } else if log::log_enabled!(log::Level::Debug) {
            log::trace!("on_completion_call: history_len={}", event.history.len());
        }

        let cancelled = self.cancel.is_cancelled();
        let ui_tx = self.ui_tx.clone();

        async move {
            if cancelled {
                return CompletionCallAction::stop("Cancelled by user");
            }
            let _ = ui_tx.send(UiEvent::LlmStart);
            CompletionCallAction::continue_run()
        }
    }

    fn on_text_delta(
        &self,
        _ctx: &HookContext,
        event: TextDelta<'_>,
    ) -> impl std::future::Future<Output = ObservationAction> + WasmCompatSend {
        let cancelled = self.cancel.is_cancelled();
        let ui_tx = self.ui_tx.clone();
        let text = event.aggregated.to_string();

        async move {
            if cancelled {
                return ObservationAction::stop("Cancelled by user");
            }
            let _ = ui_tx.send(UiEvent::AssistantMessage { text });
            ObservationAction::continue_run()
        }
    }

    fn on_tool_call(
        &self,
        _ctx: &HookContext,
        event: ToolCall<'_>,
    ) -> impl std::future::Future<Output = ToolCallAction> + WasmCompatSend {
        let tool_name = event.tool_name;
        let args = event.args;
        let tool_call_id = event.tool_call_id;

        log::trace!("on_tool_call: tool={tool_name}");

        // Pre-compute all synchronous checks and capture values for the async block
        let cancelled = self.cancel.is_cancelled();
        let subturn_action = self.subturn.check_and_increment(tool_name);
        let doom_action = self.doom.check_and_record(tool_name, args, &self.ui_tx);
        let circuit_action = self
            .circuit
            .check_server_enabled(tool_name, &self.mcp_registry);

        let source = resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry)
            .as_str()
            .to_string();

        let _ = self.ui_tx.send(UiEvent::ToolStart {
            name: tool_name.to_string(),
            source: source.clone(),
            arguments: args.to_string(),
        });

        let permission = self.permission.clone();
        let ui_tx = self.ui_tx.clone();
        let tool_name_owned = tool_name.to_string();
        let args_owned = args.to_string();
        let source_owned = source;
        let id_owned = tool_call_id.map(|s| s.to_string());

        async move {
            if cancelled {
                return ToolCallAction::stop("Cancelled by user");
            }
            if let Some(action) = subturn_action {
                return action;
            }
            if let Some(action) = doom_action {
                return action;
            }
            if let Some(action) = circuit_action {
                return action;
            }

            let decision = permission
                .resolve(&tool_name_owned, &args_owned, id_owned, Some(ui_tx.clone()))
                .await;
            match decision {
                PermissionDecision::Allow => ToolCallAction::run(),
                PermissionDecision::Deny => {
                    let _ = ui_tx.send(UiEvent::ToolEnd {
                        name: tool_name_owned,
                        source: source_owned,
                        arguments: args_owned,
                        success: false,
                        result: String::new(),
                        display: None,
                        error_kind: None,
                        message: Some("Permission denied".to_string()),
                    });
                    ToolCallAction::skip("Permission denied")
                }
            }
        }
    }

    fn on_tool_result(
        &self,
        _ctx: &HookContext,
        event: ToolResultEvent<'_>,
    ) -> impl std::future::Future<Output = ToolResultAction> + WasmCompatSend {
        let tool_name = event.tool_name;
        let args = event.args;
        let result = event.presentation;
        let result_text = result.render();

        log::trace!(
            "on_tool_result: tool={tool_name} success={} result_len={}",
            !super::agent_hook::is_tool_failure(&result_text),
            result_text.len(),
        );
        if log::log_enabled!(log::Level::Trace) {
            let preview = if result_text.len() > 2000 {
                format!(
                    "{}...<truncated {} bytes>",
                    &result_text[..2000],
                    result_text.len()
                )
            } else {
                result_text.to_string()
            };
            log::trace!("  result_body: {preview}");
        }

        // 1. Parse result JSON and extract display
        let display = serde_json::from_str::<serde_json::Value>(&result_text)
            .ok()
            .and_then(|json| crate::tools::handler::build_direct_tool_display(tool_name, &json));

        // 2. Resolve source
        let source = resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry)
            .as_str()
            .to_string();

        // 3. Classify error kind from raw result
        let error_kind = event
            .raw_result
            .error()
            .map(|e| e.kind().as_str().to_string());

        // 4. Emit ToolEnd
        let success =
            event.raw_result.error().is_none() && !super::agent_hook::is_tool_failure(&result_text);

        let _ = self.ui_tx.send(UiEvent::ToolEnd {
            name: tool_name.to_string(),
            source,
            arguments: args.to_string(),
            success,
            result: result_text.to_string(),
            display,
            error_kind,
            message: None,
        });

        // 5. Circuit breaker — track transport failures per server
        self.circuit.record_result(
            tool_name,
            &result_text,
            success,
            &self.mcp_registry,
            &self.ui_tx,
        );

        async { ToolResultAction::keep() }
    }

    fn on_stream_response_finish(
        &self,
        _ctx: &HookContext,
        event: StreamResponseFinish<'_>,
    ) -> impl std::future::Future<Output = ObservationAction> + WasmCompatSend {
        let usage = event.usage;
        if usage.total_tokens > 0 {
            let _ = self.ui_tx.send(UiEvent::LlmEnd {
                response_chars: 0,
                tool_calls: 0,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
            });
        }
        async { ObservationAction::continue_run() }
    }

    fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> impl std::future::Future<Output = Option<InvalidToolCallAction>> + WasmCompatSend {
        log::warn!("Invalid tool call: tool={}", event.tool_name);
        let feedback = format!(
            "Tool '{}' is not available. Available tools: [{}]",
            event.tool_name,
            event.available_tools.join(", ")
        );
        let _ = self.ui_tx.send(UiEvent::Warning {
            message: feedback.clone(),
        });
        async { Some(InvalidToolCallAction::retry(feedback)) }
    }
}
