use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use agnt_kernel::CompactionSource;
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
    context: Option<&str>,
    agents_chain: Option<&str>,
    available_skills: Option<&str>,
) -> Option<String> {
    log::trace!(
        "build_system_preamble: cwd={}, config_preamble={}, agent_persona={}, context={}, agents_chain={}, available_skills={}",
        cwd.is_some(),
        config_preamble.is_some(),
        agent_persona.is_some(),
        context.is_some(),
        agents_chain.is_some(),
        available_skills.is_some()
    );

    let parts: Vec<&str> = [
        cwd,
        config_preamble,
        agent_persona,
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
    runtime: tokio::runtime::Handle,
    tool_server_handle: rig::tool::server::ToolServerHandle,
    mcp_state: super::state::mcp::McpState,
    permission_state: super::state::permission::PermissionState,
    engine: Arc<dyn EngineInterfaceLike>,
    final_session_id: Option<String>,
    cwd: PathBuf,
    /// The resolved application paths.
    ///
    /// Carried from [`RuntimeBuildParams`](crate::conversation::runtime_build::RuntimeBuildParams)
    /// so an agent switch and the hook runners read the persona and config
    /// directories instead of deriving them.
    paths: crate::config::Paths,
    /// Shared pending-decision map for interactive (TUI) mode.
    ///
    /// When `Some`, `execute_turn` constructs an `InteractivePermissionResolver` instead
    /// of the default `PolicyPermissionResolver`. The main thread (orchestrator) holds
    /// a clone of this Arc and calls the map's `remove` + oneshot `send` to unblock the
    /// resolver's awaiting future after the user makes a permission decision.
    ///
    /// `None` in the composed runtime; the interactive surface installs its map
    /// with [`Self::with_decision_source`].
    interactive_pending: Option<PendingPermissions>,
    /// Whether the UI renders pre-authorize previews (`SystemEvent::PreviewRequested`).
    ///
    /// The TUI renders them and relies on the completion display being
    /// suppressed to avoid a duplicate. ACP has no preview phase — its
    /// translator drops `ToolPreview` — so the completion display must be
    /// preserved for the client to receive the diff.
    ///
    /// `false` in the composed runtime; a surface that renders them says so with
    /// [`Self::with_tool_previews`].
    renders_tool_previews: bool,
    /// Circuit breaker for MCP transport failures. Shared across turns so that
    /// consecutive failures within a session accumulate correctly.
    circuit_breaker: Arc<Mutex<McpCircuitBreaker>>,
    /// Doom loop state shared across turns so that repetitive tool call patterns
    /// are detected even when they span multiple consecutive turns.
    doom_state: Arc<Mutex<DoomLoopState>>,
    /// Assistant-output repetition state shared across turns so that consecutive
    /// text-only completions are detected even when they span multiple turns.
    output_repetition: Arc<Mutex<crate::hook::output_repetition::RepetitionState>>,
    /// Enable/disable the repetition guard (doom-loop + output-repetition
    /// detection). Resolved from config; when `false`, the hook skips all
    /// detector calls.
    repetition_guard: bool,
    /// Real token count from the last LLM completion, shared across turns: the
    /// wire's total when it reports one, else input + output. Used by the
    /// hook's compaction threshold check and the model-switch gate.
    last_total_tokens: Arc<Mutex<Option<u64>>>,
    /// The session's store-dead latch: set when a session-store open fails, so
    /// the compaction path stops attempting a store it knows is broken. Cleared
    /// by the session switch and by a `run_compaction` that opened the store.
    store_latch: crate::hook::StoreLatch,
    /// The session's cumulative LLM usage: every turn's calls accumulate here,
    /// and the cells reset only when the runtime switches sessions. The
    /// `session_ended` event reports these totals.
    session_usage: crate::hook::TurnUsage,
    /// Shared cancellation bus threaded through the turn pipeline.
    bus: crate::bus::Bus,
    /// The agent snapshot every hook envelope is stamped with.
    ///
    /// Shared with the hook runners, which read it when they build an envelope.
    /// The constructor writes the model identity; `switch_model` and
    /// `switch_agent` refresh both cells from the post-switch state.
    hook_context: crate::hooks::HookAgentContext,
    // ── Domain managers ──────────────────────────────────────────────────────
    provider: Prov,
    tools: Tools,
    session: Sess,
    persona: Persona,
    multi_agent: Multi,
    /// Shared runtime model. The single point of model identity: the agent is
    /// built from this model on every turn. It is constructed eagerly at
    /// startup and swapped on every `switch_model()` call. The compactor has
    /// its own startup-fixed model and never reads this one.
    dyn_model: Arc<Mutex<rig::DynModel<rig::operation::Completion>>>,
    /// Hook-driven compaction machinery: compactor, policy, force flag, threshold.
    compaction: crate::conversation::compaction::CompactionConfig,
}

/// Concrete production runtime: all managers use their default state types.
pub type AgentConversationRuntime = AgentRuntime<
    super::state::provider::ProviderState,
    super::state::tool::ToolState,
    super::state::memory::MemoryState,
    super::state::persona::PersonaState,
    super::state::multi_agent::MultiAgentState,
>;

impl<Prov, Tools, Sess, Persona, Multi> AgentRuntime<Prov, Tools, Sess, Persona, Multi>
where
    Prov: ProviderManager,
    Tools: ToolManager,
    Sess: SessionManager,
    Persona: PersonaManager,
    Multi: MultiAgentManager,
{
    /// The shared model slot, recovered if a switch panicked mid-swap.
    pub(crate) fn lock_dyn_model(
        &self,
    ) -> MutexGuard<'_, rig::DynModel<rig::operation::Completion>> {
        self.dyn_model
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The last completion's token count, recovered if a writer panicked.
    pub(crate) fn lock_last_total_tokens(&self) -> MutexGuard<'_, Option<u64>> {
        self.last_total_tokens
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

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
                    store_latch: Arc::clone(&self.store_latch),
                    turn_usage: crate::hook::TurnUsage::default(),
                    session_usage: self.session_usage.clone(),
                    bus: self.bus.clone(),
                    cwd: self.cwd.clone(),
                },
                Arc::clone(&self.dyn_model),
                self.compaction.clone(),
            )
            .with_agent(self.hook_context.persona().unwrap_or_default());

            let outcome = if let Some(pending) = self.interactive_pending.as_ref() {
                // TUI mode: construct InteractivePermissionResolver. The resolver owns
                // a clone of the shared bus and publishes `SystemEvent::PermissionRequested`
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
        let consumed = *self.lock_last_total_tokens();
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
        *self.lock_dyn_model() = new_model;
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
            match self.session.rig_store(&session_id).await {
                Ok(store) => {
                    if let Err(error) = crate::session::rig_state::write_state_records(
                        store,
                        identity.clone(),
                        None,
                    )
                    .await
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

        let result = self.persona.switch_agent(
            agent_name,
            &cwd,
            &self.paths.agents_dir,
            self.multi_agent.agents_config(),
        )?;

        // If persona specifies a model, attempt to switch (ignore errors)
        if let Some(model) = result.model {
            // Keep the previous config so a persona model the session does not
            // fit can be rolled back: the persona's other settings still apply.
            let previous_config = self.provider.provider_config().clone();
            let pre_override_model = self.active_model_identity();
            let switched = self.provider.switch_model(&model).is_ok();
            let consumed = *self.lock_last_total_tokens();
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
                *self.lock_dyn_model() = new_model;
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

    fn rescan_available_agents(&self) -> Vec<crate::protocol::persona::PersonaSummary> {
        use crate::protocol::persona::{FsPersonaResolver, PersonaLister};

        let resolver = FsPersonaResolver::new(
            self.cwd.clone(),
            self.paths.agents_dir.clone(),
            self.multi_agent.agents_config().clone(),
        );
        resolver.list_available()
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
        reset_session_scoped_inputs(&self.store_latch, &self.last_total_tokens);
    }

    fn new_session(&mut self) {
        self.session.clear();
        reset_session_scoped_inputs(&self.store_latch, &self.last_total_tokens);
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

    async fn run_compaction(&mut self, source: CompactionSource) -> Result<(), String> {
        let Some(session_id) = self.final_session_id.clone() else {
            return Ok(());
        };

        // Run compaction against the rig-memory store of the session: the splice
        // `compact()` makes IS what the next run loads. `run_compaction` emits
        // `Started`/`SummaryChunk`/`Completed`/`Failed` (via `compact()`).
        let _ = crate::hook::chain::run_compaction(
            &session_id,
            self.session.inner_memory(),
            &self.compaction,
            source,
            &self.last_total_tokens,
            &self.store_latch,
            self.bus.compaction(),
        )
        .await;
        Ok(())
    }

    async fn load_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        let memory = Arc::clone(self.session.memory());

        // Phase 1: exact match; Phase 2: suffix, then contains, over the
        // sessions the rig-memory store holds.
        let matched_id = if rig_holds(&memory, session_id).await? {
            session_id.to_string()
        } else {
            let sid = session_id.to_ascii_lowercase();
            let sessions = memory
                .session_ids()
                .await
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

        // Phase 3: update state and hydrate. The facts are read once; the same
        // read answers the join, the statuses, and the turns.
        let facts = rig_session_facts(&memory, &matched_id).await;
        let entries = rig_messages(&memory, &matched_id, &facts.entry_turns()).await?;
        self.session.clear();
        reset_session_scoped_inputs(&self.store_latch, &self.last_total_tokens);
        self.final_session_id = Some(matched_id.clone());
        self.session.inner_memory().warn_repairs(&matched_id).await;
        // The switch starts a new session's usage: the totals cover the
        // session the runtime is attached to, not the one it left.
        self.session_usage.reset();

        Ok(crate::session::resolver::hydrate_transcript_messages(
            &entries,
            &self.cwd,
            &facts.status_map(),
            &facts.turns,
        ))
    }

    async fn list_sessions(&self, cwd: &std::path::Path) -> Result<Vec<SessionInfo>, String> {
        let memory = Arc::clone(self.session.memory());
        let ids = memory
            .session_ids()
            .await
            .map_err(|e| format!("Failed to list sessions: {e}"))?;
        let current = self.current_session_id();
        // One facts read serves the whole listing: the agent column is the last
        // recorded turn's. A session whose facts cannot be read shows a blank
        // cell rather than a guessed one.
        let facts = match memory.facts().await {
            Ok(facts) => Some(facts),
            Err(error) => {
                log::warn!("failed to open the session fact tables for the listing: {error}");
                None
            }
        };
        let mut sessions = Vec::new();
        for id in ids {
            // A session with no messages is not usable: a model switch writes
            // its state records before the first turn, so it would otherwise
            // list as an empty, untitled session. The current session is where
            // the user already is, so it is not offered as a target.
            if current == Some(id.as_str()) {
                continue;
            }
            let Ok(store) = memory.rig_store(&id).await else {
                // One unreadable session must not hide the others.
                log::warn!("failed to open the session store of '{id}'");
                continue;
            };
            let read = crate::session::blocking::on_pool(move || {
                Ok::<rig_memory::SessionMeta, core::convert::Infallible>(
                    rig_memory::SessionMeta::read(&*store).unwrap_or_default(),
                )
            })
            .await;
            let meta = match read {
                Ok(Ok(meta)) => meta,
                Ok(Err(cannot_happen)) => match cannot_happen {},
                Err(error) => {
                    log::warn!("failed to read the session listing of '{id}': {error}");
                    continue;
                }
            };
            if meta.messages == 0 {
                continue;
            }
            // The recorded change instant, or nothing: an unknown stamp shows a
            // blank cell, never the wall clock.
            let last_active = match meta.updated {
                0 => None,
                ms => chrono::DateTime::from_timestamp_millis(ms as i64),
            };
            let agent = match facts.as_ref() {
                Some(facts) => facts.last_turn_agent(&id).await.unwrap_or_else(|error| {
                    log::warn!("failed to read the last turn of '{id}': {error}");
                    None
                }),
                None => None,
            };
            sessions.push(SessionInfo {
                id,
                message_count: meta.messages,
                last_active,
                title: (!meta.title.is_empty()).then_some(meta.title),
                agent,
            });
        }
        Ok(crate::session::prefix::filter_sessions_by_cwd(
            sessions, cwd,
        ))
    }

    async fn attach_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        let memory = Arc::clone(self.session.memory());
        // One facts read answers the keyed join, the statuses, and the turns: the
        // attach path joins exactly as the startup resolver does.
        let facts = rig_session_facts(&memory, session_id).await;
        let entries = if rig_holds(&memory, session_id).await? {
            rig_messages(&memory, session_id, &facts.entry_turns()).await?
        } else {
            Vec::new()
        };

        // Switch the runtime to the derived session. Clearing the memory cache
        // forces the next turn to re-read the session from the store, so a
        // resumed session continues from its persisted history and a fresh
        // session starts empty.
        self.session.clear();
        reset_session_scoped_inputs(&self.store_latch, &self.last_total_tokens);
        self.final_session_id = Some(session_id.to_string());
        self.session.inner_memory().warn_repairs(session_id).await;
        // The attach starts a new session's usage: the totals cover the
        // session the runtime is attached to, not the one it left.
        self.session_usage.reset();

        let (statuses, turns) = (facts.status_map(), &facts.turns);
        Ok(crate::session::resolver::hydrate_transcript_messages(
            &entries, &self.cwd, &statuses, turns,
        ))
    }
}

/// Clear the hook inputs that describe the session the runtime is leaving.
///
/// Every active-session switch (`clear_session`, `new_session`, `load_session`,
/// `attach_session`) goes through here: a store-dead latch and a last real token
/// count belong to one conversation, so neither survives a switch to another.
fn reset_session_scoped_inputs(
    store_latch: &crate::hook::StoreLatch,
    last_total_tokens: &Arc<Mutex<Option<u64>>>,
) {
    *store_latch
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    *last_total_tokens
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// Whether the rig-memory store holds a log for `session_id`.
async fn rig_holds(
    memory: &crate::session::CachedMemory,
    session_id: &str,
) -> Result<bool, String> {
    memory
        .rig_holds(session_id)
        .await
        .map_err(|e| format!("Failed to open session '{session_id}': {e}"))
}

/// The entries of `session_id`, read from the rig-memory store: each message
/// with the author the store records for it, joined to the turn that persisted
/// it by rig's key — the same keyed join the startup resolver makes.
async fn rig_messages(
    memory: &crate::session::CachedMemory,
    session_id: &str,
    turns: &std::collections::HashMap<agnt_storage::MessageKey, i64>,
) -> Result<Vec<crate::session::resolver::StoredEntry>, String> {
    let store = memory
        .rig_store(session_id)
        .await
        .map_err(|e| format!("Failed to open session '{session_id}': {e}"))?;
    let session = session_id.to_owned();
    let loaded =
        crate::session::blocking::on_pool(move || rig_memory::load(&*store, &session)).await;
    let conversation = match loaded {
        Ok(Ok(conversation)) => conversation,
        Ok(Err(e)) => return Err(format!("Failed to load session '{session_id}': {e}")),
        Err(e) => return Err(format!("Failed to load session '{session_id}': {e}")),
    };
    Ok(conversation
        .entries()
        .map(|entry| {
            let key = crate::session::settle::entry_keys([entry]);
            crate::session::resolver::StoredEntry {
                message: entry.message.clone(),
                author: entry.author.cloned(),
                turn_seq: key.first().and_then(|key| turns.get(key)).copied(),
            }
        })
        .collect())
}

/// The facts of `session_id`: the entry attributions, the call statuses, and the
/// settled turns, read once from the fact tables. A session whose facts cannot
/// be read hydrates as one that carries none: its rows read their stored
/// verdict and no entry carries a turn.
async fn rig_session_facts(
    memory: &crate::session::CachedMemory,
    session_id: &str,
) -> crate::session::settle::SessionFacts {
    use crate::session::settle::SessionFacts;
    let facts = match memory.facts().await {
        Ok(facts) => facts,
        Err(error) => {
            log::warn!("failed to open the session's fact tables for '{session_id}': {error}");
            return SessionFacts::default();
        }
    };
    match facts.read(session_id).await {
        Ok(facts) => facts,
        Err(error) => {
            log::warn!("failed to read the tool statuses for '{session_id}': {error}");
            SessionFacts::default()
        }
    }
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

    // ── Model identity and switching ────────────────────────────────

    /// The active model configuration.
    pub fn provider_config(&self) -> &crate::config::Config {
        self.provider.provider_config()
    }

    /// The active model configuration, mutably.
    ///
    /// The primitive the model-switch paths are built from:
    /// [`Self::rename_model`] changes the name through it, and a surface that
    /// simulates a provider it cannot reach (the session selector's failure
    /// path) rewrites the block here.
    pub fn provider_config_mut(&mut self) -> &mut crate::config::Config {
        self.provider.provider_config_mut()
    }

    /// Drop the cached provider client, so the next build creates a new one.
    ///
    /// [`Self::rename_model`] leaves the cache alone on purpose: the erased
    /// model is rebuilt from the cached client, which no model name changes.
    pub fn invalidate_provider_cache(&mut self) {
        self.provider.invalidate_cache();
    }

    /// Rename the active model, returning the name it replaced.
    ///
    /// A surface that swaps the model without the core switch — ACP's session
    /// selector does — rebuilds from the cached client with
    /// [`Self::build_dyn_model`] and installs the result with
    /// [`Self::replace_dyn_model`]. A failed rebuild restores the previous name
    /// with a second call. The context-window gate and the session record are
    /// deliberately absent here: [`ModelSwitching::switch_model`] is the gated
    /// switch.
    pub fn rename_model(&mut self, model: &str) -> String {
        std::mem::replace(
            &mut self.provider.provider_config_mut().model,
            model.to_string(),
        )
    }

    /// Build the erased completion model from the cached client.
    pub async fn build_dyn_model(
        &mut self,
    ) -> Result<rig::DynModel<rig::operation::Completion>, LabeledError> {
        self.provider.build_dyn_model().await
    }

    /// Install `model` as the runtime's shared model.
    pub fn replace_dyn_model(&self, model: rig::DynModel<rig::operation::Completion>) {
        *self.lock_dyn_model() = model;
    }

    /// The engine handle the composition root installed.
    ///
    /// The part is carried for the surfaces that execute Nu closures against it;
    /// no path in this crate reads it back.
    pub fn engine(&self) -> Arc<dyn EngineInterfaceLike> {
        Arc::clone(&self.engine)
    }

    /// The agent snapshot every hook envelope is stamped with.
    pub fn hook_context(&self) -> &crate::hooks::HookAgentContext {
        &self.hook_context
    }

    // ── Session surface ──────────────────────────────────────────────

    /// A handle on the runtime's bus.
    ///
    /// The ACP session manager holds it to publish and subscribe on the
    /// session's channels without locking the runtime. Handing the whole bus
    /// out is the shape ARCH-8 counts; a session manager that took only the
    /// channels it uses would not need this accessor, so narrowing it belongs
    /// to the bus slice.
    pub fn bus_handle(&self) -> crate::bus::Bus {
        self.bus.clone()
    }

    /// The session's cumulative usage cells, shared with the runtime.
    ///
    /// A clone of the handle reads the same cells; unlike
    /// [`SessionPersistence::session_usage`] it needs no runtime lock.
    pub fn session_usage_handle(&self) -> crate::hook::TurnUsage {
        self.session_usage.clone()
    }

    /// The persona directory the agent selector reads.
    pub fn agents_dir(&self) -> &std::path::Path {
        &self.paths.agents_dir
    }

    /// The resolved application paths.
    pub fn paths(&self) -> &crate::config::Paths {
        &self.paths
    }

    /// The agents config the persona resolver reads.
    pub fn agents_config(&self) -> &crate::config::AgentsConfig {
        self.multi_agent.agents_config()
    }

    // ── Tools and permissions ────────────────────────────────────────

    /// The tool definitions the model can see: registered tools filtered by the
    /// MCP registry and the permission policy.
    pub fn visible_tool_definitions(&self) -> Vec<crate::types::ToolDefinition> {
        self.tools.active_definitions(
            self.mcp_state.mcp_registry(),
            self.permission_state.permissions(),
        )
    }

    /// The permission policy in force.
    pub fn permissions(&self) -> &crate::tools::authz::PermissionsConfig {
        self.permission_state.permissions()
    }

    /// The session's grant cache, shared with the policy in force.
    pub fn session_grants(&self) -> Arc<Mutex<crate::tools::authz::SessionGrantCache>> {
        self.permission_state.session_grants_arc()
    }

    /// Every tool definition registered with the runtime, before the MCP and
    /// permission filters.
    pub fn tool_definitions(&self) -> &[crate::types::ToolDefinition] {
        self.tools.tool_definitions()
    }

    /// Log the permission policy summary once for this session.
    ///
    /// [`CoreRuntime::execute_turn`] calls it before the first turn; a surface
    /// that reports the policy before any turn runs calls it itself.
    pub fn emit_permission_summary(&mut self) {
        self.permission_state.emit_startup_summary_once();
    }

    // ── Compaction and persona ───────────────────────────────────────

    /// The hook-driven compaction machinery: compactor, policy and threshold.
    pub fn compaction(&self) -> &crate::conversation::compaction::CompactionConfig {
        &self.compaction
    }

    /// The persona body the active agent resolved, when it has one.
    pub fn agent_persona_body(&self) -> Option<&str> {
        self.persona.agent_persona_body()
    }

    /// The cached `AGENTS.md` chain the preamble carries, when loaded.
    pub fn cached_agents_chain(&self) -> Option<&str> {
        self.persona.cached_agents_chain()
    }

    // ── End Phase I accessor methods ────────────────────────────────
}

// region:    --- Constructors

use super::runtime_build::RuntimeBuildParams;
use super::runtime_parts::RuntimeParts;

impl AgentConversationRuntime {
    /// Compose the runtime from the parts a composition root supplies and the
    /// configuration only this constructor knows how to apply.
    ///
    /// `parts` carries what cannot be derived — provider, tools, session,
    /// persona, multi-agent state, engine, bus — one named field each, so a
    /// composition site that omits a part does not compile. Everything else is
    /// built here: the erased models, the MCP and permission state, the hook
    /// context and the compaction config.
    pub async fn compose(
        parts: RuntimeParts,
        config: RuntimeBuildParams,
    ) -> Result<Self, LabeledError> {
        use super::compaction::compactor::NuCompactor;
        use super::runtime_build::{build_compaction_policy, resolve_light_config};
        use super::state::mcp::McpState;
        use super::state::permission::PermissionState;
        use super::state::provider::ProviderState;
        use crate::config::defaults;
        use rig::DynModel;
        use rig::operation::Completion;

        let RuntimeParts {
            mut provider,
            tools,
            session,
            persona,
            multi,
            engine,
            bus,
        } = parts;
        // The active model configuration. `RuntimeBuildParams` carries it in its
        // own `config` field; every model identity below is derived from it.
        let active_config = config.config;

        // Extract max_tool_result_bytes before the config is consumed below.
        let max_tool_result_bytes = active_config
            .max_tool_result_bytes
            .unwrap_or(defaults::MAX_TOOL_RESULT_BYTES);
        // The hook envelopes stamp the model the runtime starts on. Every switch
        // refreshes it from the post-switch state.
        let hook_context = config.hook_context;
        hook_context.set_model(crate::protocol::picker::format_active_model_identity(
            &active_config.provider,
            &active_config.model,
        ));
        // Resolve the context-window limit before the config is consumed below.
        // Used to compute the hook's compaction threshold.
        let max_context_tokens = active_config
            .model_context_tokens
            .or_else(|| active_config.max_context_tokens.map(|t| t as usize))
            .unwrap_or(defaults::MAX_CONTEXT_TOKENS as usize);
        // Resolve the repetition-guard flag before the config is consumed below:
        // when unset, the guard defaults to enabled.
        let repetition_guard = active_config
            .repetition_guard
            .unwrap_or(defaults::REPETITION_GUARD_ENABLED);
        // The compaction (summarizer) model: the `[models.light]` role when it
        // resolves, else the startup active model. A caller that builds the
        // compactor itself hands it in `compactor_model`, and the compaction
        // identity is then the active model. It is chosen once, here, and never
        // follows a model switch.
        let supplied_compactor = config.compactor_model;
        let light_config = if supplied_compactor.is_some() {
            None
        } else {
            resolve_light_config(config.plugin_config.as_ref())
        };
        // The planning spec describes the COMPACTION model: rig-memory sizes each
        // summarized portion to its context window.
        let compaction_config = light_config.as_ref().unwrap_or(&active_config);
        let compaction_source = if light_config.is_some() {
            "[models.light]"
        } else {
            "the startup active model"
        };
        log::info!(
            "compaction model: {}/{} ({compaction_source})",
            compaction_config.provider,
            compaction_config.model
        );
        let compaction_spec =
            crate::conversation::compaction::compactor::resolve_model_spec_for(compaction_config);
        // The compactor's provider needs the plugin config after the agent
        // provider takes ownership of it.
        let compactor_plugin_config = config.plugin_config.clone();
        // The compaction policy: rig-memory's default with the `[compaction]`
        // block's overrides on top.
        let compaction_config = config
            .plugin_config
            .as_ref()
            .and_then(|plugin_config| plugin_config.compaction.clone())
            .unwrap_or_default();
        let compaction_policy = build_compaction_policy(&compaction_config);

        // The shared model is the single point of model identity: the agent is
        // built from it on every turn. It is constructed eagerly at startup so it
        // is ready for any operation (including `/compact` before the first turn),
        // and rewritten by every `switch_model()`.
        let dyn_model = provider
            .build_dyn_model()
            .await
            .map_err(|e| LabeledError::new(format!("Failed to build model: {e}")))?;
        let dyn_model: Arc<Mutex<DynModel<Completion>>> = Arc::new(Mutex::new(dyn_model));

        // Compactor summarizes evicted messages with its own erased model: the
        // light role's when configured, else the startup active model. It is
        // startup-fixed — no model switch touches it — and it is a standalone
        // service invoked directly by the hook's compaction logic.
        let compactor_model: Arc<Mutex<DynModel<Completion>>> = match supplied_compactor {
            Some(compactor_model) => compactor_model,
            None => {
                let compactor_model = match light_config {
                    Some(light_config) => {
                        let mut light_provider =
                            ProviderState::new(light_config, compactor_plugin_config)
                                .with_copilot_token_dir(config.paths.copilot_token_dir.clone());
                        light_provider.build_dyn_model().await.map_err(|e| {
                            LabeledError::new(format!("Failed to build compactor model: {e}"))
                        })?
                    }
                    None => provider.build_dyn_model().await.map_err(|e| {
                        LabeledError::new(format!("Failed to build compactor model: {e}"))
                    })?,
                };
                Arc::new(Mutex::new(compactor_model))
            }
        };
        let compactor = NuCompactor::from_shared_model(
            Arc::clone(&compactor_model),
            bus.clone(),
            compaction_policy,
            compaction_spec,
        );

        // Surface what reading the session repaired, such as an image whose blob is
        // gone. A session that cannot be read still opens; the next save reports it.
        if let Some(session_id) = config.final_session_id.as_deref() {
            session.warn_repairs(session_id).await;
        }

        // Threshold used by the hook to decide when to compact: proactive_threshold_pct
        // (default 0.8) of model_context_tokens (falling back to max_context_tokens).
        let threshold_pct = config
            .proactive_threshold_pct
            .unwrap_or(defaults::PROACTIVE_THRESHOLD_PCT);
        let threshold_tokens = Some((max_context_tokens as f64 * threshold_pct) as usize);

        let compaction = crate::conversation::compaction::CompactionConfig {
            compactor,
            threshold_tokens,
            threshold_pct,
        };

        Ok(Self {
            runtime: config.runtime,
            tool_server_handle: config.tool_server_handle,
            provider,
            tools,
            mcp_state: McpState::new(
                config.mcp_runtime,
                config.mcp_lifecycle_projection,
                config.mcp_server_configs,
                config.mcp_caller_cwd,
                config.mcp_registry,
                max_tool_result_bytes,
                config.vault,
            ),
            engine,
            final_session_id: config.final_session_id,
            permission_state: PermissionState::new(
                config.base_permissions,
                config.effective_permissions,
                config.cli_permissions_overlay,
                crate::tools::authz::SessionGrantCache::default(),
                config.permissions_startup_summary,
            ),
            session,
            persona,
            multi_agent: multi,
            cwd: config.cwd,
            paths: config.paths,
            interactive_pending: None,
            renders_tool_previews: false,
            circuit_breaker: Arc::new(Mutex::new(McpCircuitBreaker::default())),
            doom_state: Arc::new(Mutex::new(crate::hook::DoomLoopState::default())),
            output_repetition: Arc::new(Mutex::new(
                crate::hook::output_repetition::RepetitionState::default(),
            )),
            repetition_guard,
            last_total_tokens: Arc::new(Mutex::new(None)),
            bus,
            hook_context,
            session_usage: crate::hook::TurnUsage::default(),
            store_latch: Default::default(),
            dyn_model,
            compaction,
        })
    }
}

// endregion: --- Constructors

// region:    --- Seams

impl crate::protocol::contracts::DecisionSource for AgentConversationRuntime {
    /// Set the permission decision source of the composed runtime.
    ///
    /// `Some` installs the interactive surface's bridge: the gate publishes a
    /// request and parks the call until the surface answers on the map. `None`
    /// keeps the pre-determined policy, where `ask` resolves to deny.
    fn with_decision_source(self, resolver: Option<PendingPermissions>) -> Self {
        Self {
            interactive_pending: resolver,
            ..self
        }
    }

    /// Declare that this surface renders pre-authorize previews.
    ///
    /// A surface that draws them says so here, at composition time, so the turn
    /// can publish `SystemEvent::PreviewRequested` and suppress the duplicate
    /// completion display. Surfaces with no preview phase leave the composed
    /// default (`false`).
    fn with_tool_previews(self, renders: bool) -> Self {
        Self {
            renders_tool_previews: renders,
            ..self
        }
    }
}

impl crate::protocol::contracts::RuntimeAccess for AgentConversationRuntime {
    fn paths(&self) -> &crate::config::Paths {
        Self::paths(self)
    }

    fn hook_context(&self) -> &crate::hooks::HookAgentContext {
        Self::hook_context(self)
    }

    fn bus_handle(&self) -> crate::bus::Bus {
        Self::bus_handle(self)
    }

    fn provider_name(&self) -> &str {
        Self::provider_name(self)
    }

    fn model(&self) -> &str {
        Self::model(self)
    }

    fn mcp_caller_cwd(&self) -> Option<&std::path::Path> {
        Self::mcp_caller_cwd(self)
    }

    fn mcp_lifecycle_projection(&self) -> &[crate::tools::mcp::runtime::McpServerLifecycle] {
        Self::mcp_lifecycle_projection(self)
    }
}

// endregion: --- Seams

#[cfg(test)]
#[path = "../../test/conversation/runtime.rs"]
mod runtime_test;
