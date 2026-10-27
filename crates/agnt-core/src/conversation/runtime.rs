use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nu_protocol::{LabeledError, Span, Value};

use crate::bus::{Bus, SessionEvent};
use crate::hook::agent_hook::DoomLoopState;
use crate::protocol::contracts::UiMessageSnapshot;
use crate::session::SessionInfo;
use crate::tools::closure::EngineInterfaceLike;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;

use super::managers::{
    MultiAgentManager, PersonaManager, ProviderManager, SessionManager, ToolManager,
};
use crate::protocol::event::PermissionDecision as ProtocolPermissionDecision;

use crate::bus::OneshotTx;

/// Shared pending-permission map for TUI mode: maps request IDs to oneshot senders
/// that unblock the `InteractivePermissionResolver` awaiting a user decision.
pub type PendingPermissions = Arc<Mutex<HashMap<String, OneshotTx<ProtocolPermissionDecision>>>>;
use crate::protocol::{
    contracts::{CoreRuntime, McpUsabilityState},
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
    pub engine: Arc<dyn EngineInterfaceLike>,
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
    /// Whether the UI renders pre-authorize previews (`UiEvent::ToolPreview`).
    ///
    /// The TUI renders them and relies on the completion display being
    /// suppressed to avoid a duplicate. ACP has no preview phase — its
    /// translator drops `ToolPreview` — so the completion display must be
    /// preserved for the client to receive the diff.
    ///
    /// Defaults to `false` in `build_runtime`; TUI mode sets it to `true`.
    pub renders_tool_previews: bool,
    /// Circuit breaker for MCP transport failures. Shared across turns so that
    /// consecutive failures within a session accumulate correctly.
    pub circuit_breaker: Arc<Mutex<McpCircuitBreaker>>,
    /// Doom loop state shared across turns so that repetitive tool call patterns
    /// are detected even when they span multiple consecutive turns.
    pub doom_state: Arc<Mutex<DoomLoopState>>,
    /// Assistant-output repetition state shared across turns so that consecutive
    /// text-only completions are detected even when they span multiple turns.
    pub output_repetition: Arc<Mutex<crate::hook::output_repetition::RepetitionState>>,
    /// Enable/disable the repetition guard (doom-loop + output-repetition
    /// detection). Resolved from config; when `false`, the hook skips all
    /// detector calls.
    pub repetition_guard: bool,
    /// Real token count from the last LLM completion, shared across turns: the
    /// wire's total when it reports one, else input + output. Used by the
    /// hook's compaction threshold check and the model-switch gate.
    pub last_total_tokens: Arc<Mutex<Option<u64>>>,
    /// The session's cumulative LLM usage: every turn's calls accumulate here,
    /// and the cells reset only when the runtime switches sessions. The
    /// `session_ended` event reports these totals.
    pub session_usage: crate::hook::TurnUsage,
    /// Shared cancellation bus threaded through the turn pipeline.
    pub bus: crate::bus::Bus,
    /// Per-tool call-line render functions, populated at builtin registration
    /// time and threaded to the hook chain via `ToolInfra`.
    pub render_registry: crate::tools::handler::builtin_tool::ToolRenderRegistry,
    /// The agent snapshot every hook envelope is stamped with.
    ///
    /// Shared with the hook runners, which read it when they build an envelope.
    /// The constructor writes the model identity; `switch_model` and
    /// `switch_agent` refresh both cells from the post-switch state.
    pub hook_context: crate::hooks::HookAgentContext,
    // ── Domain managers ──────────────────────────────────────────────────────
    pub provider: Prov,
    pub tools: Tools,
    pub session: Sess,
    pub persona: Persona,
    pub multi_agent: Multi,
    /// Shared runtime model. The single point of model identity: the agent is
    /// built from this model on every turn. It is constructed eagerly at
    /// startup and swapped on every `switch_model()` call. The compactor has
    /// its own startup-fixed model and never reads this one.
    pub dyn_model: Arc<Mutex<rig::DynModel<rig::operation::Completion>>>,
    /// Hook-driven compaction machinery: compactor, policy, force flag, threshold.
    pub compaction: crate::conversation::compaction::CompactionConfig,
}

/// Concrete production runtime: all managers use their default state types.
pub type AgentConversationRuntime = AgentRuntime<
    super::state::provider::ProviderState,
    super::state::tool::ToolState,
    super::state::memory::MemoryState,
    super::state::persona::PersonaState,
    super::state::multi_agent::MultiAgentState,
>;

impl<Prov, Tools, Sess, Persona, Multi> CoreRuntime
    for AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager + Send,
    Tools: ToolManager + Send,
    Sess: SessionManager<
            InnerMemory = crate::session::CachedMemory,
            Memory = super::state::memory::MemoryOf,
        > + Send,
    Persona: PersonaManager + Send,
    Multi: MultiAgentManager + Send,
{
    async fn execute_turn(
        &mut self,
        _bus: &Bus,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        use super::turn::executor::{
            ExecuteInput, ToolInfra, TurnExecutor, TurnOutcome, build_response,
        };
        use crate::hook::{InteractivePermissionResolver, PolicyPermissionResolver};

        self.permission_state.emit_startup_summary_once();

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
        self.provider.ensure_client_cached().await?;

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
                    output_repetition: Arc::clone(&self.output_repetition),
                    repetition_guard: self.repetition_guard,
                    last_total_tokens: Arc::clone(&self.last_total_tokens),
                    turn_usage: crate::hook::TurnUsage::default(),
                    session_usage: self.session_usage.clone(),
                    bus: self.bus.clone(),
                    render_registry: self.render_registry.clone(),
                    cwd: self.cwd.clone(),
                },
                Arc::clone(&self.dyn_model),
                self.compaction.clone(),
            );

            let outcome = if let Some(pending) = self.interactive_pending.as_ref() {
                // TUI mode: construct InteractivePermissionResolver. The resolver owns
                // a clone of the shared bus and publishes `UiEvent::PermissionRequested`
                // on `bus.ui_event()` directly. No UI channel is threaded through the
                // executor; the turn's drain loop creates its own channel internally
                // and terminates when the prompt resolves.
                let resolver = InteractivePermissionResolver::new(
                    Arc::clone(pending),
                    Arc::clone(&permissions),
                    Arc::clone(&session_grants),
                    Arc::clone(&closure_registry),
                    Arc::clone(&mcp_registry),
                    self.cwd.clone(),
                )
                .with_renders_tool_previews(self.renders_tool_previews);
                executor
                    .execute(
                        ExecuteInput {
                            prompt,
                            preamble,
                            span,
                        },
                        resolver,
                        self.final_session_id.as_deref(),
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
                        ExecuteInput {
                            prompt,
                            preamble,
                            span,
                        },
                        permission_resolver,
                        self.final_session_id.as_deref(),
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

impl<Prov, Tools, Sess, Persona, Multi> AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager + Send,
    Tools: ToolManager + Send,
    Sess: SessionManager + Send,
    Persona: PersonaManager + Send,
    Multi: MultiAgentManager + Send,
{
    /// Recompute the compaction threshold against the active model's context
    /// window, using the build's precedence: `model_context_tokens`, then
    /// `max_context_tokens`, then the built-in default.
    ///
    /// The planning spec is deliberately left alone: it describes the
    /// startup-fixed compaction model, not the active one.
    fn refresh_compaction_threshold(&mut self) {
        let max_context_tokens = {
            let config = self.provider.provider_config();
            config
                .model_context_tokens
                .or_else(|| config.max_context_tokens.map(|t| t as usize))
                .unwrap_or(crate::config::defaults::MAX_CONTEXT_TOKENS as usize)
        };
        self.compaction.threshold_tokens =
            Some((max_context_tokens as f64 * self.compaction.threshold_pct) as usize);
    }
}

impl<Prov, Tools, Sess, Persona, Multi> ModelSwitching
    for AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager + Send,
    Tools: ToolManager + Send,
    Sess: SessionManager + Send,
    Persona: PersonaManager + Send,
    Multi: MultiAgentManager + Send,
{
    async fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        // The identity in use before the switch, so the switch event can report
        // where the session came from.
        let from = self.active_model_identity();
        // Keep the previous config so a switch the session does not fit can be
        // rolled back: the resolver has already committed the new one.
        let previous_config = self.provider.provider_config().clone();
        let identity = self.provider.switch_model(model_spec)?;
        let max_tokens = self.max_context_tokens();
        // A session that already consumed the target's whole window cannot fit
        // the next turn, so the switch is refused rather than breaking it. An
        // unknown consumed count or window means nothing to protect.
        let consumed = *self.last_total_tokens.lock().expect("model mutex poisoned");
        if let Some(consumed) = consumed
            && let Some(window) = self.max_context_tokens()
            && consumed >= window
        {
            *self.provider.provider_config_mut() = previous_config;
            return Err(format!(
                "cannot switch to '{identity}': the session has consumed about {consumed} tokens and the model's context window is {window} tokens; compact the session first (/compact)"
            ));
        }
        // Erase the newly-cached concrete model into a `DynModel<Completion>`
        // and swap it into the shared slot. The next turn builds its agent from
        // the new model; an in-flight turn keeps the model it already cloned.
        // The model is built from the bare model name via the canonical
        // constructor.
        let new_model = self
            .provider
            .build_dyn_model()
            .await
            .map_err(|e| e.to_string())?;
        *self.dyn_model.lock().expect("model mutex poisoned") = new_model;
        // The threshold is an absolute token count, so it must follow the new
        // model's context window.
        self.refresh_compaction_threshold();
        // Hook envelopes follow the active model.
        self.hook_context.set_model(self.active_model_identity());
        // Every successful switch publishes, so a hook observes the change. The
        // send is best effort: the switch already took effect, and a closed bus
        // must not undo it. Both failure returns above sit before this point.
        let _ = self
            .bus
            .session()
            .send(SessionEvent::ModelSwitched {
                from,
                to: self.active_model_identity(),
            })
            .await;
        // Record the new model on the session so a listing shows it. The write
        // is best effort: the switch already took effect, and a store that
        // cannot be written must not undo it.
        if let Some(session_id) = self.final_session_id.clone() {
            match self.session.rig_store(&session_id) {
                Ok(store) => {
                    if let Err(error) =
                        crate::session::rig_state::write_state_records(&*store, &identity, None)
                    {
                        log::warn!(
                            "failed to write the model state record of '{session_id}': {error}"
                        );
                    }
                }
                Err(error) => {
                    log::warn!("failed to open the session store of '{session_id}': {error}");
                }
            }
        }
        Ok((identity, max_tokens))
    }

    async fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
        // The persona in use before the switch, so the switch event can report
        // where the session came from. `None` when no persona was loaded.
        let agent_from = self.hook_context.persona();
        // The identity the persona's model override replaced, when one took
        // effect. The override is published as its own switch event.
        let mut model_from: Option<String> = None;
        let cwd = self
            .mcp_state
            .mcp_caller_cwd()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "agent switch unavailable: working directory not set".to_string())?;

        let result =
            self.persona
                .switch_agent(agent_name, &cwd, self.multi_agent.agents_config())?;

        // If persona specifies a model, attempt to switch (ignore errors)
        if let Some(model) = result.model {
            // Keep the previous config so a persona model the session does not
            // fit can be rolled back: the persona's other settings still apply.
            let previous_config = self.provider.provider_config().clone();
            let pre_override_model = self.active_model_identity();
            let switched = self.provider.switch_model(&model).is_ok();
            let consumed = *self.last_total_tokens.lock().expect("model mutex poisoned");
            let blocked = match (switched, consumed, self.max_context_tokens()) {
                (true, Some(consumed), Some(window)) if consumed >= window => {
                    Some((consumed, window))
                }
                _ => None,
            };
            if let Some((consumed, window)) = blocked {
                *self.provider.provider_config_mut() = previous_config;
                log::warn!(
                    "skipping the persona model '{model}': the session has consumed about \
                     {consumed} tokens and the model's context window is {window} tokens; \
                     compact the session first (/compact)"
                );
            } else if let Ok(new_model) = self.provider.build_dyn_model().await {
                // Rebuild the shared model from the bare model name so the
                // persona's model serves the next turn (mirrors switch_model).
                *self.dyn_model.lock().expect("model mutex poisoned") = new_model;
                self.refresh_compaction_threshold();
            }
            // The override took effect when the provider accepted it and the
            // window gate did not roll it back: the persona's model is what the
            // session now runs, so it is its own switch event. A dyn-model
            // rebuild failure does not undo that: the config already committed.
            if switched && blocked.is_none() {
                model_from = Some(pre_override_model);
            }
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

        // Hook envelopes follow the new persona slug and the model it resolved
        // to: `switch_agent` applies the persona's model override, so both
        // cells are read from the post-switch state.
        self.hook_context.set_persona(Some(agent_name.to_string()));
        self.hook_context.set_model(self.active_model_identity());

        // Every successful switch publishes, so a hook observes the change.
        // Both sends are best effort: the switch already took effect. A persona
        // whose model override took effect changed two facts, so it publishes
        // two events — the agent first, then the model — and each envelope
        // reports the post-switch state it was stamped from.
        let _ = self
            .bus
            .session()
            .send(SessionEvent::AgentSwitched {
                from: agent_from,
                to: agent_name.to_string(),
            })
            .await;
        if let Some(from) = model_from {
            let _ = self
                .bus
                .session()
                .send(SessionEvent::ModelSwitched {
                    from,
                    to: self.active_model_identity(),
                })
                .await;
        }

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
        // A new session starts with no usage of its own.
        self.session_usage.reset();
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
    Sess: SessionManager<
            InnerMemory = crate::session::CachedMemory,
            Memory = super::state::memory::MemoryOf,
        > + Send
        + Sync,
    Persona: PersonaManager + Send + Sync,
    Multi: MultiAgentManager + Send + Sync,
{
    fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    fn current_session_id(&self) -> Option<&str> {
        self.final_session_id.as_deref()
    }

    fn session_usage(&self) -> crate::hook::TurnUsageTotals {
        self.session_usage.totals()
    }

    async fn run_compaction(&mut self, source: &str) -> Result<(), String> {
        let Some(session_id) = self.final_session_id.clone() else {
            return Ok(());
        };

        // Run compaction against the rig-memory store of the session: the splice
        // `compact()` makes IS what the next run loads. `source` is `"auto"` or
        // `"slash"`. `run_compaction` emits `Started`/`SummaryChunk`/`Completed`/
        // `Failed` (via `compact()`).
        let _ = crate::hook::chain::run_compaction(
            &session_id,
            self.session.inner_memory(),
            &self.compaction,
            source,
            &self.last_total_tokens,
            &self.bus,
        )
        .await;
        Ok(())
    }

    async fn load_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        let memory = Arc::clone(self.session.memory());

        // Phase 1: exact match; Phase 2: suffix, then contains, over the
        // sessions the rig-memory store holds.
        let matched_id = if rig_holds(&memory, session_id)? {
            session_id.to_string()
        } else {
            let sid = session_id.to_ascii_lowercase();
            let sessions = memory
                .session_ids()
                .map_err(|e| format!("Failed to list sessions: {e}"))?;
            sessions
                .iter()
                .find(|id| id.to_ascii_lowercase().ends_with(&sid))
                .or_else(|| {
                    sessions
                        .iter()
                        .find(|id| id.to_ascii_lowercase().contains(&sid))
                })
                .cloned()
                .ok_or_else(|| format!("Session '{session_id}' not found"))?
        };

        // Phase 3: update state and hydrate
        let messages = rig_messages(&memory, &matched_id)?;
        self.session.clear();
        self.final_session_id = Some(matched_id.clone());
        self.session.inner_memory().warn_repairs(&matched_id);
        // The switch starts a new session's usage: the totals cover the
        // session the runtime is attached to, not the one it left.
        self.session_usage.reset();

        Ok(crate::session::resolver::hydrate_transcript_messages(
            &messages, &self.cwd,
        ))
    }

    async fn list_sessions(&self, cwd: &std::path::Path) -> Result<Vec<SessionInfo>, String> {
        let memory = Arc::clone(self.session.memory());
        let ids = memory
            .session_ids()
            .map_err(|e| format!("Failed to list sessions: {e}"))?;
        let current = self.current_session_id();
        let mut sessions = Vec::new();
        for id in ids {
            // A session with no messages is not usable: a model switch writes
            // its state records before the first turn, so it would otherwise
            // list as an empty, untitled session. The current session is where
            // the user already is, so it is not offered as a target.
            if current == Some(id.as_str()) {
                continue;
            }
            let Ok(store) = memory.rig_store(&id) else {
                // One unreadable session must not hide the others.
                log::warn!("failed to open the session store of '{id}'");
                continue;
            };
            let meta = rig_memory::SessionMeta::read(&*store).unwrap_or_default();
            if meta.messages == 0 {
                continue;
            }
            let last_active = match meta.updated {
                0 => chrono::Utc::now(),
                ms => chrono::DateTime::from_timestamp_millis(ms as i64)
                    .unwrap_or_else(chrono::Utc::now),
            };
            sessions.push(SessionInfo {
                id,
                message_count: meta.messages,
                last_active,
                title: (!meta.title.is_empty()).then_some(meta.title),
            });
        }
        Ok(crate::session::prefix::filter_sessions_by_cwd(
            sessions, cwd,
        ))
    }

    async fn attach_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        let memory = Arc::clone(self.session.memory());
        let messages = if rig_holds(&memory, session_id)? {
            rig_messages(&memory, session_id)?
        } else {
            Vec::new()
        };

        // Switch the runtime to the derived session. Clearing the memory cache
        // forces the next turn to re-read the session from the store, so a
        // resumed session continues from its persisted history and a fresh
        // session starts empty.
        self.session.clear();
        self.final_session_id = Some(session_id.to_string());
        self.session.inner_memory().warn_repairs(session_id);
        // The attach starts a new session's usage: the totals cover the
        // session the runtime is attached to, not the one it left.
        self.session_usage.reset();

        Ok(crate::session::resolver::hydrate_transcript_messages(
            &messages, &self.cwd,
        ))
    }
}

/// Whether the rig-memory store holds a log for `session_id`.
fn rig_holds(memory: &crate::session::CachedMemory, session_id: &str) -> Result<bool, String> {
    memory
        .rig_holds(session_id)
        .map_err(|e| format!("Failed to open session '{session_id}': {e}"))
}

/// The messages of `session_id`, read from the rig-memory store.
fn rig_messages(
    memory: &crate::session::CachedMemory,
    session_id: &str,
) -> Result<Vec<crate::types::Message>, String> {
    let store = memory
        .rig_store(session_id)
        .map_err(|e| format!("Failed to open session '{session_id}': {e}"))?;
    let conversation = rig_memory::load(&*store, session_id)
        .map_err(|e| format!("Failed to load session '{session_id}': {e}"))?;
    Ok(conversation
        .messages()
        .iter()
        .map(|message| (**message).clone())
        .collect())
}

impl<Prov, Tools, Sess, Persona, Multi> AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager<
            InnerMemory = crate::session::CachedMemory,
            Memory = super::state::memory::MemoryOf,
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
    /// can hold its own `Arc` clone and manipulate the map when the user delivers
    /// a permission decision through the TUI (see `PermissionStage`).
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
#[path = "../../test/conversation/runtime.rs"]
mod runtime_test;
