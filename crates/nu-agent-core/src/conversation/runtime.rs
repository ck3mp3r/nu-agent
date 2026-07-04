use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::hook::agent_hook::DoomLoopState;
use crate::session::SessionStore;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

use super::compaction::CompactionGuard;
use super::compaction::executor::CompactionExecutor;
use super::managers::{
    CompactionManager, MultiAgentManager, PersonaManager, ProviderManager, SessionManager,
    ToolManager,
};
use crate::protocol::event::PermissionDecision as ProtocolPermissionDecision;

/// Shared pending-permission map for TUI mode: maps request IDs to oneshot senders
/// that unblock the `InteractivePermissionResolver` awaiting a user decision.
pub type PendingPermissions =
    Arc<Mutex<HashMap<String, oneshot::Sender<ProtocolPermissionDecision>>>>;
use crate::protocol::{
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    compaction_runtime::HasCompaction,
    contracts::{CoreRuntime, McpUsabilityState, ProgressUi},
    event::UiEvent,
    mcp_management::HasMcpManagement,
    model_switching::HasModelSwitching,
    session_management::HasSessionManagement,
};

/// Build system preamble from components.
/// Joins non-empty parts with separators. Returns None if all empty.
fn build_system_preamble(
    config_preamble: Option<&str>,
    agent_persona: Option<&str>,
    sub_agent_instruction: Option<&str>,
    context: Option<&str>,
    agents_chain: Option<&str>,
    available_skills: Option<&str>,
) -> Option<String> {
    log::trace!(
        "build_system_preamble: config_preamble={}, agent_persona={}, sub_agent_instruction={}, context={}, agents_chain={}, available_skills={}",
        config_preamble.is_some(),
        agent_persona.is_some(),
        sub_agent_instruction.is_some(),
        context.is_some(),
        agents_chain.is_some(),
        available_skills.is_some()
    );

    let parts: Vec<&str> = [
        config_preamble,
        agent_persona,
        sub_agent_instruction,
        context,
        agents_chain,
        available_skills,
    ]
    .into_iter()
    .flatten()
    .collect();

    if parts.is_empty() {
        None
    } else {
        log::debug!(
            "build_system_preamble: parts_count={}, total_len={}",
            parts.len(),
            parts.iter().map(|p| p.len()).sum::<usize>()
        );
        Some(parts.join("\n\n---\n\n"))
    }
}

/// Generic agent runtime composed of independent domain managers.
///
/// Each generic parameter is a manager type that owns one domain of state.
/// The concrete infrastructure fields (tokio runtime, MCP state, permissions,
/// session store) remain as named fields because they are not interchangeable.
///
/// Use the `AgentConversationRuntime` type alias for the concrete production type.
pub struct AgentRuntime<Prov, Tools, Sess, Comp, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Comp: CompactionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    // ── Concrete infrastructure ──────────────────────────────────────────────
    pub runtime: tokio::runtime::Runtime,
    pub tool_server_handle: rig::tool::server::ToolServerHandle,
    pub mcp_state: super::state::mcp::McpState,
    pub permission_state: super::state::permission::PermissionState,
    pub engine: EngineInterface,
    pub store: SessionStore,
    pub final_session_id: Option<String>,
    pub cwd: PathBuf,
    /// Shared pending-decision map for interactive (TUI) mode.
    ///
    /// When `Some`, `execute_turn` constructs an `InteractivePermissionResolver` instead
    /// of the default `PolicyPermissionResolver`. The main thread (orchestrator) holds
    /// a clone of this Arc and calls the map's `remove` + oneshot `send` to unblock the
    /// resolver's awaiting future after the user makes a permission decision.
    ///
    /// Set to `None` in TTY mode (default from `build_runtime`).
    pub interactive_pending: Option<PendingPermissions>,
    /// Circuit breaker for MCP transport failures. Shared across turns so that
    /// consecutive failures within a session accumulate correctly.
    pub circuit_breaker: Arc<Mutex<McpCircuitBreaker>>,
    /// Doom loop state shared across turns so that repetitive tool call patterns
    /// are detected even when they span multiple consecutive turns.
    pub doom_state: Arc<Mutex<DoomLoopState>>,
    // ── Domain managers ──────────────────────────────────────────────────────
    pub provider: Prov,
    pub tools: Tools,
    pub session: Sess,
    pub compaction: Comp,
    pub persona: Persona,
    pub multi_agent: Multi,
}

/// Concrete production runtime: all managers use their default state types.
pub type AgentConversationRuntime = AgentRuntime<
    super::state::provider::ProviderState,
    super::state::tool::ToolState,
    super::state::memory::MemoryState,
    super::compaction::state::CompactionState,
    super::state::persona::PersonaState,
    super::state::multi_agent::MultiAgentState,
>;

impl<Prov, Tools, Sess, Comp, Persona, Multi> CoreRuntime
    for AgentRuntime<Prov, Tools, Sess, Comp, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Comp: CompactionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        use super::turn::executor::{
            ExecuteInput, ToolInfra, TurnExecutor, TurnOutcome, build_response,
        };
        use crate::hook::{InteractivePermissionResolver, PolicyPermissionResolver};

        self.permission_state.emit_startup_summary_once(ui);

        // Build system preamble from cached components
        let preamble = build_system_preamble(
            self.provider.provider_config().preamble.as_deref(),
            self.persona.agent_persona_body(),
            self.persona.cached_sub_agent_instruction(),
            context.as_deref(),
            self.persona.cached_agents_chain(),
            self.persona.cached_available_skills(),
        );

        log::debug!(
            "execute_turn: preamble present={}, preamble_len={}",
            preamble.is_some(),
            preamble.as_ref().map_or(0, |p| p.len())
        );

        // Provider dispatch - build model from cached client
        self.provider.ensure_client_cached()?;

        // Compute filtered tool definitions before entering the mutable borrow scope
        let visible_tool_definitions = self.tools.active_definitions(
            self.mcp_state.mcp_registry(),
            self.permission_state.permissions(),
        );

        let closure_registry = Arc::new(self.tools.closure_registry().clone());
        let mcp_registry = Arc::new(self.mcp_state.mcp_registry().clone());
        let permissions = Arc::new(self.permission_state.permissions().clone());
        let session_grants = self.permission_state.session_grants_arc();

        let cached_client = self
            .provider
            .cached_client()
            .expect("client must be cached before execute_turn");

        // Scope the executor so its borrows are released before compaction.
        let (outcome, response_data) = {
            let mut executor = TurnExecutor::new(
                self.provider.provider_config(),
                &self.runtime,
                &mut self.session,
                ToolInfra {
                    closure_registry: Arc::clone(&closure_registry),
                    mcp_registry: Arc::clone(&mcp_registry),
                    tool_server_handle: self.tool_server_handle.clone(),
                    visible_tool_definitions,
                    circuit_breaker: Arc::clone(&self.circuit_breaker),
                    doom_state: Arc::clone(&self.doom_state),
                },
            );

            let outcome = if let Some(pending) = self.interactive_pending.as_ref() {
                // TUI mode: construct InteractivePermissionResolver.
                //
                // Create the tokio UI event channel here so that we can:
                //   1. Pass the (ui_tx, ui_rx) pair to the executor so the drain loop uses
                //      the same channel that the hook's ui_tx writes events to.
                //   2. The resolver does NOT own a ui_tx — it receives one per-call from
                //      the AgentHook, preventing the executor's stack-held resolver from
                //      keeping a sender alive that would deadlock the drain loop.
                let (ui_tx, ui_rx) = mpsc::unbounded_channel::<UiEvent>();
                let resolver = InteractivePermissionResolver::new(
                    Arc::clone(pending),
                    Arc::clone(&permissions),
                    Arc::clone(&session_grants),
                    Arc::clone(&closure_registry),
                    Arc::clone(&mcp_registry),
                );
                executor.execute(
                    ui,
                    ExecuteInput {
                        prompt,
                        preamble,
                        span,
                    },
                    cached_client,
                    resolver,
                    self.final_session_id.as_deref(),
                    Some((ui_tx, ui_rx)),
                )
            } else {
                // TTY mode: use PolicyPermissionResolver (immediate Allow/Deny from config).
                let permission_resolver = PolicyPermissionResolver {
                    permissions: Arc::clone(&permissions),
                    session_grants: Arc::clone(&session_grants),
                    closure_registry: Arc::clone(&closure_registry),
                    mcp_registry: Arc::clone(&mcp_registry),
                };
                executor.execute(
                    ui,
                    ExecuteInput {
                        prompt,
                        preamble,
                        span,
                    },
                    cached_client,
                    permission_resolver,
                    self.final_session_id.as_deref(),
                    None,
                )
            };

            // Extract response data before executor is dropped
            let response_data = executor.take_response_data();
            (outcome, response_data)
        };

        match outcome? {
            TurnOutcome::EarlyReturn(value) => Ok(value),
            TurnOutcome::Completed => {
                // Evaluate auto-compaction (runtime method) after turn completes
                if self.final_session_id.is_some()
                    && let Some(CompactionTriggerDecision::Fire { source, .. }) =
                        self.evaluate_auto_compaction()
                    && let Err(error) = self.execute_compaction_event(ui, source)
                {
                    ui.emit(&UiEvent::Warning { message: error });
                }

                Ok(build_response(
                    response_data,
                    self.provider.provider_config(),
                    self.final_session_id.as_deref(),
                    span,
                ))
            }
        }
    }
}

impl<Prov, Tools, Sess, Comp, Persona, Multi> HasMcpManagement
    for AgentRuntime<Prov, Tools, Sess, Comp, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Comp: CompactionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    fn set_mcp_server_enabled(
        &mut self,
        name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        let handle = self.tool_server_handle.clone();
        self.mcp_state.set_mcp_server_enabled(
            &handle,
            name,
            enabled,
            self.tools.tool_definitions_mut(),
            &self.runtime.handle().clone(),
        )
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.mcp_state
            .llm_visible_mcp_tool_count(&self.tools.active_definitions(
                self.mcp_state.mcp_registry(),
                self.permission_state.permissions(),
            ))
    }

    fn llm_visible_mcp_tool_count_for_server(&self, server_name: &str) -> usize {
        self.mcp_state.llm_visible_mcp_tool_count_for_server(
            server_name,
            &self.tools.active_definitions(
                self.mcp_state.mcp_registry(),
                self.permission_state.permissions(),
            ),
        )
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        self.mcp_state
            .llm_visible_mcp_tool_names_by_server(&self.tools.active_definitions(
                self.mcp_state.mcp_registry(),
                self.permission_state.permissions(),
            ))
    }
}

impl<Prov, Tools, Sess, Comp, Persona, Multi> HasModelSwitching
    for AgentRuntime<Prov, Tools, Sess, Comp, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Comp: CompactionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        let identity = self.provider.switch_model(model_spec)?;
        let max_tokens = self.max_context_tokens();
        Ok((identity, max_tokens))
    }

    fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
        let cwd = self
            .mcp_state
            .mcp_caller_cwd()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "agent switch unavailable: working directory not set".to_string())?;

        let result =
            self.persona
                .switch_agent(agent_name, &cwd, self.multi_agent.agents_config())?;

        // If persona specifies a model, attempt to switch (ignore errors)
        if let Some(ref model) = result.model {
            let _ = self.provider.switch_model(model);
        }

        // Reset tool definitions to baseline on agent switch
        self.tools.reset_to_baseline();

        // Invalidate cached client to pick up any changes
        self.provider.invalidate_cache();

        Ok(result.identity)
    }

    fn active_model_identity(&self) -> String {
        self.persona
            .active_model_identity(self.provider.provider_config())
    }

    fn max_context_tokens(&self) -> Option<u64> {
        AgentRuntime::<Prov, Tools, Sess, Comp, Persona, Multi>::max_context_tokens(self)
    }
}

impl<Prov, Tools, Sess, Comp, Persona, Multi> HasCompaction
    for AgentRuntime<Prov, Tools, Sess, Comp, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Comp: CompactionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        self.compaction
            .evaluate_auto_compaction(self.session.last_total_tokens())
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        self.execute_compaction_event(ui, source)
    }
}

impl<Prov, Tools, Sess, Comp, Persona, Multi> HasSessionManagement
    for AgentRuntime<Prov, Tools, Sess, Comp, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Comp: CompactionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    fn clear_session(&mut self) {
        self.session.clear();
    }

    fn new_session(&mut self) {
        self.session.clear();
        let new_id = crate::session::resolver::generate_session_id();
        let prefix = crate::session::prefix::dir_prefix(&self.cwd);
        let new_id = format!("{prefix}-{new_id}");
        self.final_session_id = Some(new_id.clone());
        let _ = self.store.get_or_create(Some(new_id));
    }

    fn seed_last_total_tokens(&mut self, tokens: Option<u64>) {
        *self.session.last_total_tokens_mut() = tokens;
    }
}

impl<Prov, Tools, Sess, Comp, Persona, Multi> AgentRuntime<Prov, Tools, Sess, Comp, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Comp: CompactionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    // ── Phase I accessor methods ────────────────────────────────────

    /// Spawn a future on the tokio runtime.
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }

    /// Return a clone of the interactive-mode pending-decision map.
    ///
    /// The orchestrator's main thread calls this before spawning the worker so it
    /// can hold its own `Arc` clone and call `submit_decision` (or directly manipulate
    /// the map) when the user delivers a permission decision through the TUI.
    ///
    /// Returns `None` in TTY mode.
    pub fn interactive_pending_arc(&self) -> Option<PendingPermissions> {
        self.interactive_pending.as_ref().map(Arc::clone)
    }

    pub fn provider_name(&self) -> &str {
        &self.provider.provider_config().provider
    }

    pub fn model(&self) -> &str {
        &self.provider.provider_config().model
    }

    pub fn max_context_tokens(&self) -> Option<u64> {
        self.provider
            .provider_config()
            .max_context_tokens
            .map(u64::from)
    }

    pub fn startup_plugin_config(&self) -> Option<&crate::config::PluginConfig> {
        self.provider.startup_plugin_config()
    }

    pub fn agent_identity(&self) -> Option<&str> {
        self.persona.agent_identity()
    }

    pub fn persona_body_len(&self) -> Option<usize> {
        self.persona.persona_body_len()
    }

    pub fn agent_description(&self) -> Option<&str> {
        self.persona.agent_description()
    }

    pub fn mcp_caller_cwd(&self) -> Option<&std::path::Path> {
        self.mcp_state.mcp_caller_cwd()
    }

    pub fn mcp_lifecycle_projection(&self) -> &[crate::tools::mcp::runtime::McpServerLifecycle] {
        self.mcp_state.mcp_lifecycle_projection()
    }

    pub fn available_agent_summaries(&self) -> &[crate::protocol::persona::PersonaSummary] {
        self.multi_agent.available_agent_summaries()
    }

    pub fn take_mailbox_rx(
        &mut self,
    ) -> Option<std::sync::mpsc::Receiver<crate::mailbox::IncomingMessage>> {
        self.multi_agent.take_mailbox_rx()
    }

    // ── End Phase I accessor methods ────────────────────────────────

    fn execute_compaction_event<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        if self
            .compaction
            .compacting()
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Ok(());
        }
        let _guard = CompactionGuard(Arc::clone(self.compaction.compacting()));

        self.provider
            .ensure_client_cached()
            .map_err(|e| e.to_string())?;

        let session_id = self
            .final_session_id
            .as_ref()
            .ok_or_else(|| "session_unavailable".to_string())?;

        let result = CompactionExecutor::new(
            self.provider.provider_config(),
            &self.runtime,
            self.session.memory(),
            &self.store,
            session_id,
        )
        .execute(
            ui,
            source,
            self.provider
                .cached_client()
                .expect("client must be cached during compaction"),
        )?;

        if let Some(summary_total_tokens) = result {
            // Use the summary token count captured from the streaming Final chunk.
            // If the provider didn't yield usage, fall back to None so the stale
            // pre-compaction count doesn't re-trigger compaction on next load.
            *self.session.last_total_tokens_mut() = summary_total_tokens;
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
