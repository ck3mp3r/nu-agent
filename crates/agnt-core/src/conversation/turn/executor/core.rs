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

/// User-facing marker published when the user cancels a turn. The TUI renders
/// it as a brief status-bar marker rather than a transcript block. The turn's
/// completed work is kept in the session log, so the stop was deliberate and
/// nothing already done was lost.
pub const TURN_STOPPED_NOTICE: &str = "Turn stopped";

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nu_protocol::{LabeledError, Span, Value};

use crate::bus::Bus;
use crate::config::Config;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::managers::SessionManager;
use crate::hook::agent_hook::DoomLoopState;
use crate::hook::output_repetition::RepetitionState;
use crate::hook::permission_resolver::AsyncPermissionResolver;
use crate::session::CachedMemory;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;
use crate::types::ToolDefinition;

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
    /// Per-tool call-line render functions, populated at builtin registration
    /// time and consulted by the hook chain to build the transcript call line.
    pub render_registry: ToolRenderRegistry,
    /// The session working directory. The hook chain reads it to build the
    /// argument-sourced preview for a call the turn stopped before it ran.
    pub cwd: PathBuf,
}

pub struct TurnExecutor<'a, S: SessionManager> {
    pub config: &'a Config,
    pub memory_state: &'a mut S,
    pub tool_infra: ToolInfra,
    /// Shared runtime model used to build each turn's agent. Cloned into every
    /// `TurnConversation`. It is constructed eagerly at startup and swapped by
    /// `switch_model()`.
    pub dyn_model: Arc<Mutex<rig::DynModel<rig::operation::Completion>>>,
    /// Hook-driven compaction machinery: compactor, policy, force flag, threshold.
    pub compaction: CompactionConfig,
    /// Stored after a completed turn so the delegate can extract it for response formatting.
    pub(super) response_data: Option<TurnResponseData>,
}

impl<'a, S> TurnExecutor<'a, S>
where
    S: SessionManager<
            Memory = crate::conversation::state::memory::MemoryOf,
            InnerMemory = CachedMemory,
        >,
{
    pub fn new(
        config: &'a Config,
        memory_state: &'a mut S,
        tool_infra: ToolInfra,
        dyn_model: Arc<Mutex<rig::DynModel<rig::operation::Completion>>>,
        compaction: CompactionConfig,
    ) -> Self {
        Self {
            config,
            memory_state,
            tool_infra,
            dyn_model,
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
}
