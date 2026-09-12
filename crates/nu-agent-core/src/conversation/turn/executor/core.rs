//! Owns the logic for executing a single agent turn: building the rig agent,
//! dispatching through the hook, handling cancel paths, and persisting results.
//!
//! Extracted from `AgentConversationRuntime::execute_turn` to give it a single
//! responsibility. `AgentConversationRuntime` constructs a `TurnExecutor` and delegates.

/// Statement appended to the user-facing error when steering is exhausted and
/// the provider returned no partial output. The partial response text is not
/// carried in the error path, so this explicit no-output statement satisfies
/// the exhausted-steering contract.
pub const NO_OUTPUT_STATEMENT: &str = "No output was produced.";

/// User-facing notice published when the executor converts a repetition stop
/// into a steering retry (Path A or Path C). The steering itself rides
/// memory, so without this notice the retry is invisible and the user only
/// sees the eventual terminal stop.
pub const REPETITION_STEERING_NOTICE: &str =
    "Repetition guard steering the model to try a different approach...";

use std::sync::{Arc, Mutex};

use nu_protocol::{LabeledError, Span, Value};
use rig::memory::ConversationMemory;

use crate::bus::{Bus, WarningEvent};
use crate::config::Config;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::managers::SessionManager;
use crate::hook::agent_hook::DoomLoopState;
use crate::hook::output_repetition::RepetitionState;
use crate::hook::permission_resolver::AsyncPermissionResolver;
use crate::session::{CachedMemory, SessionStore};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;
use crate::types::{Message, ToolDefinition};

/// Outcome of `TurnExecutor::execute` — either a completed turn whose results
/// have been persisted and whose UI events have been emitted, or an early-exit
/// value (cancelled / error) that the caller can return directly.
#[derive(Debug)]
pub enum TurnOutcome {
    /// The turn completed normally. The delegate should evaluate auto-compaction
    /// and then build the final `Value` using `build_response`.
    Completed,
    /// Early exit — the caller should return the contained `Value` directly.
    /// Used for cancellation paths where a minimal response is returned.
    EarlyReturn(Value),
}

/// Bundles the per-turn input parameters for [`TurnExecutor::execute`].
pub struct ExecuteInput {
    pub prompt: String,
    pub preamble: Option<String>,
    pub span: Span,
}

/// Data captured during a completed turn, used by `build_response` to construct
/// the final `Value`. Allows the response to be built after the executor's
/// borrows are released (so the delegate can run compaction in between).
pub struct TurnResponseData {
    pub text: String,
    pub usage: rig::completion::request::Usage,
    pub has_session: bool,
}

/// Groups the tool infrastructure fields always passed through to TurnContext.
#[derive(Clone)]
pub struct ToolInfra {
    pub closure_registry: Arc<ClosureRegistry>,
    pub mcp_registry: Arc<McpToolRegistry>,
    pub tool_server_handle: rig::tool::server::ToolServerHandle,
    pub visible_tool_definitions: Vec<ToolDefinition>,
    pub circuit_breaker: Arc<Mutex<McpCircuitBreaker>>,
    pub doom_state: Arc<Mutex<DoomLoopState>>,
    /// Session-scoped assistant-output repetition state, shared across turns and
    /// caller retries so consecutive text-only completions accumulate across
    /// executor attempts.
    pub output_repetition: Arc<Mutex<RepetitionState>>,
    /// Enable/disable the repetition guard (doom-loop + output-repetition
    /// detection). Passed through to the hook chain.
    pub repetition_guard: bool,
    /// Real token count from the last LLM completion, shared across turns. Used by
    /// the hook's compaction threshold check.
    pub last_total_tokens: Arc<Mutex<Option<u64>>>,
    /// Shared cancellation bus threaded through the turn pipeline.
    pub bus: Bus,
}

pub struct TurnExecutor<
    'a,
    S: SessionManager,
    ST: SessionStore + Clone + Send + Sync = crate::session::SessionStoreBackend,
> {
    pub config: &'a Config,
    pub memory_state: &'a mut S,
    pub tool_infra: ToolInfra,
    /// Shared runtime model handle used to build each turn's agent and route
    /// model selection. Cloned into every `TurnConversation`. It is constructed
    /// eagerly at startup.
    pub shared_model: Arc<Mutex<rig::agent::ModelHandle>>,
    /// Hook-driven compaction machinery: compactor, policy, force flag, threshold.
    pub compaction: CompactionConfig<ST>,
    /// Stored after a completed turn so the delegate can extract it for response formatting.
    pub(super) response_data: Option<TurnResponseData>,
}

impl<'a, ST, S> TurnExecutor<'a, S, ST>
where
    ST: SessionStore + Clone + Send + Sync + 'static,
    S: SessionManager<
            Memory = crate::conversation::state::memory::MemoryOf<ST>,
            InnerMemory = CachedMemory<ST>,
        >,
{
    pub fn new(
        config: &'a Config,
        memory_state: &'a mut S,
        tool_infra: ToolInfra,
        shared_model: Arc<Mutex<rig::agent::ModelHandle>>,
        compaction: CompactionConfig<ST>,
    ) -> Self {
        Self {
            config,
            memory_state,
            tool_infra,
            shared_model,
            compaction,
            response_data: None,
        }
    }

    /// Extract the response data captured during `execute`. Returns `None` if
    /// `execute` was not called or did not complete normally.
    pub fn take_response_data(&mut self) -> Option<TurnResponseData> {
        self.response_data.take()
    }

    /// Execute the turn: dispatch through the provider visitor, handle cancellation
    /// paths, persist messages, and emit UI events. Returns `TurnOutcome::Completed`
    /// on success (caller should then evaluate compaction and call `build_response`),
    /// or `TurnOutcome::EarlyReturn(value)` for cancellation paths.
    pub async fn execute<P: AsyncPermissionResolver>(
        &mut self,
        input: ExecuteInput,
        permission_resolver: P,
        final_session_id: Option<&str>,
    ) -> Result<TurnOutcome, LabeledError> {
        let prompt = input.prompt;
        let preamble = input.preamble;
        let span = input.span;
        let conversation_id = if let Some(session_id) = final_session_id {
            session_id.to_string()
        } else {
            // No session: use transient ID based on timestamp
            format!(
                "transient-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            )
        };

        // Save tool definitions for retry (std::mem::take consumes them on each attempt)
        let saved_tool_definitions = self.tool_infra.visible_tool_definitions.clone();

        let visitor_result = self
            .run_retry_loop(
                prompt.clone(),
                preamble,
                conversation_id,
                saved_tool_definitions,
                final_session_id,
                permission_resolver,
            )
            .await;

        self.dispatch_result(visitor_result, prompt, span, final_session_id)
            .await
    }

    /// Append messages to the session memory, surfacing failures instead of
    /// dropping them silently. A failed append means this turn's messages are
    /// NOT in the session store, so the failure is logged at error level and
    /// surfaced to the user as a `WarningEvent::Message` on the bus warning
    /// channel. `label` names the append site for the log and the warning.
    pub(super) async fn append_to_memory_or_warn(
        &mut self,
        session_id: &str,
        messages: Vec<Message>,
        label: &str,
    ) {
        if let Err(mem_err) = self
            .memory_state
            .memory_mut()
            .append(session_id, messages)
            .await
        {
            log::error!("Failed to append session memory ({label}): {mem_err}");
            let _ = self
                .tool_infra
                .bus
                .warning()
                .send(WarningEvent::Message {
                    message: format!(
                        "Session memory update failed: {label} — this turn may not be saved."
                    ),
                })
                .await;
        }
    }
}
