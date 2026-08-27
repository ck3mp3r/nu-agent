use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::hook::agent_hook::DoomLoopState;
use crate::protocol::contracts::UiMessageSnapshot;
use crate::session::SessionInfo;
use crate::session::SessionStore;
use crate::session::SessionStoreBackend;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

use super::managers::{
    MultiAgentManager, PersonaManager, ProviderManager, SessionManager, ToolManager,
};
use crate::protocol::event::PermissionDecision as ProtocolPermissionDecision;

/// Shared pending-permission map for TUI mode: maps request IDs to oneshot senders
/// that unblock the `InteractivePermissionResolver` awaiting a user decision.
pub type PendingPermissions =
    Arc<Mutex<HashMap<String, oneshot::Sender<ProtocolPermissionDecision>>>>;
use crate::protocol::{
    contracts::{CoreRuntime, McpUsabilityState, ProgressUi},
    event::UiEvent,
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
};

/// Build system preamble from components.
/// Joins non-empty parts with separators. Returns None if all empty.
fn build_system_preamble(
    cwd: Option<&str>,
    config_preamble: Option<&str>,
    agent_persona: Option<&str>,
    sub_agent_instruction: Option<&str>,
    context: Option<&str>,
    agents_chain: Option<&str>,
    available_skills: Option<&str>,
) -> Option<String> {
    log::trace!(
        "build_system_preamble: cwd={}, config_preamble={}, agent_persona={}, sub_agent_instruction={}, context={}, agents_chain={}, available_skills={}",
        cwd.is_some(),
        config_preamble.is_some(),
        agent_persona.is_some(),
        sub_agent_instruction.is_some(),
        context.is_some(),
        agents_chain.is_some(),
        available_skills.is_some()
    );

    let parts: Vec<&str> = [
        cwd,
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
pub struct AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    // ── Concrete infrastructure ──────────────────────────────────────────────
    pub runtime: tokio::runtime::Handle,
    pub tool_server_handle: rig::tool::server::ToolServerHandle,
    pub mcp_state: super::state::mcp::McpState,
    pub permission_state: super::state::permission::PermissionState,
    pub engine: EngineInterface,
    pub store: Arc<SessionStoreBackend>,
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
    /// Real token count from the last LLM completion, shared across turns. Used by
    /// the hook's compaction threshold check.
    pub last_total_tokens: Arc<Mutex<Option<u64>>>,
    /// Shared cancellation bus threaded through the turn pipeline.
    pub bus: crate::bus::Bus,
    // ── Domain managers ──────────────────────────────────────────────────────
    pub provider: Prov,
    pub tools: Tools,
    pub session: Sess,
    pub persona: Persona,
    pub multi_agent: Multi,
    /// Shared runtime model handle. The single point of model identity: both the
    /// agent (built via `AgentBuilder::from_model_handle`) and the hook's
    /// `on_model_select` route to this handle's current value. It is constructed
    /// eagerly at startup and updated on every `switch_model()` call.
    pub shared_model: Arc<Mutex<rig::agent::ModelHandle>>,
    /// Hook-driven compaction machinery: compactor, policy, force flag, threshold.
    pub compaction: crate::conversation::compaction::CompactionConfig<SessionStoreBackend>,
}

/// Concrete production runtime: all managers use their default state types.
pub type AgentConversationRuntime = AgentRuntime<
    super::state::provider::ProviderState,
    super::state::tool::ToolState,
    super::state::memory::MemoryState<SessionStoreBackend>,
    super::state::persona::PersonaState,
    super::state::multi_agent::MultiAgentState,
>;

impl<Prov, Tools, Sess, Persona, Multi> CoreRuntime
    for AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager + Send,
    Tools: ToolManager + Send,
    Sess: SessionManager<
            InnerMemory = crate::session::CachedMemory<SessionStoreBackend>,
            Memory = super::state::memory::MemoryOf<SessionStoreBackend>,
        > + Send,
    Persona: PersonaManager + Send,
    Multi: MultiAgentManager + Send,
{
    async fn execute_turn<U: ProgressUi + Send>(
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
        let cwd_str = self.cwd.to_str().map(|s| format!("Working directory: {s}"));
        let preamble = build_system_preamble(
            cwd_str.as_deref(),
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

        // Scope the executor so its borrows are released before compaction.
        let (outcome, response_data) = {
            let mut executor = TurnExecutor::new(
                self.provider.provider_config(),
                &mut self.session,
                ToolInfra {
                    closure_registry: Arc::clone(&closure_registry),
                    mcp_registry: Arc::clone(&mcp_registry),
                    tool_server_handle: self.tool_server_handle.clone(),
                    visible_tool_definitions,
                    circuit_breaker: Arc::clone(&self.circuit_breaker),
                    doom_state: Arc::clone(&self.doom_state),
                    last_total_tokens: Arc::clone(&self.last_total_tokens),
                    bus: self.bus.clone(),
                },
                Arc::clone(&self.shared_model),
                self.compaction.clone(),
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
                executor
                    .execute(
                        ui,
                        ExecuteInput {
                            prompt,
                            preamble,
                            span,
                        },
                        resolver,
                        self.final_session_id.as_deref(),
                        Some((ui_tx, ui_rx)),
                    )
                    .await
            } else {
                // TTY mode: use PolicyPermissionResolver (immediate Allow/Deny from config).
                let permission_resolver = PolicyPermissionResolver {
                    permissions: Arc::clone(&permissions),
                    session_grants: Arc::clone(&session_grants),
                    closure_registry: Arc::clone(&closure_registry),
                    mcp_registry: Arc::clone(&mcp_registry),
                };
                executor
                    .execute(
                        ui,
                        ExecuteInput {
                            prompt,
                            preamble,
                            span,
                        },
                        permission_resolver,
                        self.final_session_id.as_deref(),
                        None,
                    )
                    .await
            };

            // Extract response data before executor is dropped
            let response_data = executor.take_response_data();
            (outcome, response_data)
        };

        match outcome? {
            TurnOutcome::EarlyReturn(value) => Ok(value),
            TurnOutcome::Completed => Ok(build_response(
                response_data,
                self.provider.provider_config(),
                self.final_session_id.as_deref(),
                span,
            )),
        }
    }
}

impl<Prov, Tools, Sess, Persona, Multi> McpManagement
    for AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager + Send,
    Tools: ToolManager + Send,
    Sess: SessionManager + Send,
    Persona: PersonaManager + Send,
    Multi: MultiAgentManager + Send,
{
    async fn set_mcp_server_enabled(
        &mut self,
        name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        if !enabled {
            self.permission_state.clear_session_grants_for_server(name);
        }
        let handle = self.tool_server_handle.clone();
        self.mcp_state
            .set_mcp_server_enabled(&handle, name, enabled, self.tools.tool_definitions_mut())
            .await
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

impl<Prov, Tools, Sess, Persona, Multi> ModelSwitching
    for AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        let identity = self.provider.switch_model(model_spec)?;
        let max_tokens = self.max_context_tokens();
        // Erase the newly-cached concrete model into a `ModelHandle` and update
        // the shared handle. One write updates both the hook (`on_model_select`)
        // and the compactor (`NuCompactor::from_shared_model`) since they share
        // this `Arc`.
        self.provider
            .ensure_client_cached()
            .map_err(|e| e.to_string())?;
        let new_model = self
            .provider
            .cached_client()
            .ok_or_else(|| "client must be cached after model switch".to_string())?
            .build_model_handle(&identity)
            .map_err(|e| e.to_string())?;
        *self.shared_model.lock().expect("model mutex poisoned") = new_model;
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

        // Apply persona config overrides for in-session switch.
        // On an explicit switch, persona values REPLACE the previous config values (not guarded by is_none()).
        // Fields absent from the new persona (None) leave the current config unchanged.
        if let Some(t) = result.temperature {
            self.provider.provider_config_mut().temperature = Some(t);
        }
        if let Some(m) = result.max_tokens {
            self.provider.provider_config_mut().max_tokens = Some(m);
        }
        if let Some(t) = result.max_tool_turns {
            self.provider.provider_config_mut().max_tool_turns = Some(t);
        }
        if let Some(c) = result.max_tool_calls_per_subturn {
            self.provider
                .provider_config_mut()
                .max_tool_calls_per_subturn = Some(c);
        }
        if let Some(b) = result.max_tool_result_bytes {
            self.provider.provider_config_mut().max_tool_result_bytes = Some(b);
        }
        if let Some(p) = result.additional_params {
            self.provider.provider_config_mut().additional_params = Some(p);
        }

        // Apply permission overlay from agent frontmatter.
        // When the new persona has no `permissions:` block, the overlay is
        // empty, effectively resetting to the base config + CLI flags.
        // This ensures that switching from a restrictive persona to one
        // without explicit permissions restores the default access.
        let overlay = result
            .permissions_overlay
            .as_ref()
            .cloned()
            .unwrap_or_default();
        self.permission_state.with_agent_overlay(&overlay);

        // Clear session grants so "Allow always" from the previous agent
        // does not persist into the new agent.
        self.permission_state.clear_session_grants();

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
        self.provider
            .provider_config()
            .max_context_tokens
            .map(u64::from)
    }

    fn agent_description(&self) -> Option<&str> {
        self.persona.agent_description()
    }

    fn agent_icon(&self) -> Option<&str> {
        self.persona.agent_icon()
    }
}

impl<Prov, Tools, Sess, Persona, Multi> SessionState
    for AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
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
        self.final_session_id = Some(new_id);
    }

    fn seed_last_total_tokens(&mut self, tokens: Option<u64>) {
        *self.session.last_total_tokens_mut() = tokens;
    }
}

impl<Prov, Tools, Sess, Persona, Multi> SessionPersistence
    for AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager + Send + Sync,
    Tools: ToolManager + Send + Sync,
    Sess: SessionManager<InnerMemory = crate::session::CachedMemory<SessionStoreBackend>>
        + Send
        + Sync,
    Persona: PersonaManager + Send + Sync,
    Multi: MultiAgentManager + Send + Sync,
{
    fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    async fn run_compaction(&mut self, source: &str) -> Result<(), String> {
        let Some(session_id) = self.final_session_id.clone() else {
            return Ok(());
        };

        // Load the full history from the store-backed memory. The `CachedMemory`
        // cache may be stale (the store is the source of truth for compaction),
        // so read the raw entries directly.
        let memory = self.session.inner_memory();
        let entries = memory
            .load_all(&session_id)
            .await
            .map_err(|e| format!("Failed to load session '{session_id}': {e}"))?;
        let history: Vec<crate::types::Message> = entries
            .iter()
            .filter_map(|e| match e {
                crate::session::StoreEntry::Message(m) => Some(m.clone()),
                _ => None,
            })
            .collect();

        // Run compaction. `source` is `"auto"` or `"slash"`. `run_compaction`
        // emits `Started`/`SummaryChunk`/`Completed`/`Failed` (via `compact()`)
        // and writes the marker. It returns the patched history
        // (`Some([summary_message])`) when compaction actually ran, or `None`
        // when there was nothing to evict.
        if let Some(patched) = crate::hook::chain::run_compaction(
            &history,
            &session_id,
            memory,
            &self.compaction,
            source,
            &self.last_total_tokens,
            &self.bus,
        )
        .await
        {
            // Update the in-memory cache so the next `CachedMemory::load()`
            // returns `[summary, ...new_messages]` as the conversation continues.
            self.session
                .inner_memory()
                .reset_context(&session_id, patched);
        }
        Ok(())
    }

    async fn load_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        let store = Arc::clone(&self.store);
        let sid = session_id.to_string();

        // Phase 1: try exact match
        let result = store
            .load(&sid)
            .await
            .map_err(|e| format!("Failed to load session '{session_id}': {e}"))?;

        // Phase 2: prefix/contains match if exact failed
        let (metadata, entries) = match result {
            Some(data) => data,
            None => {
                let sessions = store
                    .list()
                    .await
                    .map_err(|e| format!("Failed to list sessions: {e}"))?;

                let matched = sessions
                    .iter()
                    .find(|s| {
                        s.id.to_ascii_lowercase()
                            .ends_with(&sid.to_ascii_lowercase())
                    })
                    .or_else(|| {
                        sessions.iter().find(|s| {
                            s.id.to_ascii_lowercase()
                                .contains(&sid.to_ascii_lowercase())
                        })
                    })
                    .ok_or_else(|| format!("Session '{session_id}' not found"))?;

                let matched_id = matched.id.clone();
                store
                    .load(&matched_id)
                    .await
                    .map_err(|e| format!("Failed to load session '{}': {e}", matched.id))?
                    .ok_or_else(|| format!("Session '{}' not found", matched.id))?
            }
        };

        // Phase 3: update state and hydrate
        self.session.clear();
        self.final_session_id = Some(metadata.session_id.clone());

        Ok(crate::session::resolver::hydrate_transcript_from_store_entries(&entries))
    }

    async fn list_sessions(&self, cwd: &std::path::Path) -> Result<Vec<SessionInfo>, String> {
        let sessions = self
            .store
            .list()
            .await
            .map_err(|e| format!("Failed to list sessions: {e}"))?;
        Ok(crate::session::prefix::filter_sessions_by_cwd(
            sessions, cwd,
        ))
    }
}

impl<Prov, Tools, Sess, Persona, Multi> AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager<
            InnerMemory = crate::session::CachedMemory<SessionStoreBackend>,
            Memory = super::state::memory::MemoryOf<SessionStoreBackend>,
        >,
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

    pub fn agent_icon(&self) -> Option<&str> {
        self.persona.agent_icon()
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

    // ── End Phase I accessor methods ────────────────────────────────
}

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
