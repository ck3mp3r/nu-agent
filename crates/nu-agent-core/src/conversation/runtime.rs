use std::sync::Arc;
use std::sync::atomic::Ordering;

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};

use crate::session::SessionStore;

use super::compaction::CompactionGuard;
use super::compaction::executor::CompactionExecutor;
use crate::protocol::{
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    contracts::{CoreRuntime, ExtendedRuntime, McpUsabilityState, ProgressUi},
    event::UiEvent,
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

pub struct AgentConversationRuntime {
    pub runtime: tokio::runtime::Runtime,
    pub provider_state: super::state::provider::ProviderState,
    pub tool_state: super::state::tool::ToolState,
    pub mcp_state: super::state::mcp::McpState,
    pub engine: EngineInterface,
    pub store: SessionStore,
    pub final_session_id: Option<String>,
    pub compaction_state: super::compaction::state::CompactionState,
    pub permission_state: super::state::permission::PermissionState,
    pub memory_state: super::state::memory::MemoryState,
    pub persona_state: super::state::persona::PersonaState,
    pub multi_agent_state: super::state::multi_agent::MultiAgentState,
}

impl CoreRuntime for AgentConversationRuntime {
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

        self.permission_state.emit_startup_summary_once(ui);

        // Build system preamble from cached components
        let preamble = build_system_preamble(
            self.provider_state.config().preamble.as_deref(),
            self.persona_state.agent_persona_body(),
            self.persona_state.cached_sub_agent_instruction(),
            context.as_deref(),
            self.persona_state.cached_agents_chain(),
            self.persona_state.cached_available_skills(),
        );

        log::debug!(
            "execute_turn: preamble present={}, preamble_len={}",
            preamble.is_some(),
            preamble.as_ref().map_or(0, |p| p.len())
        );

        // Hydrate memory from conversation store (idempotent, guarded)
        self.ensure_memory_hydrated()?;

        // Provider dispatch - build model from cached client
        self.provider_state.ensure_client_cached()?;

        // Compute filtered tool definitions before entering the mutable borrow scope
        let visible_tool_definitions = self.tool_state.active_definitions(
            self.mcp_state.mcp_registry(),
            self.permission_state.permissions(),
        );

        // Take the client temporarily to avoid overlapping borrows with `self`.
        let cached_client = self.provider_state.take_client().unwrap();

        // Scope the executor so its borrows are released before compaction.
        let (outcome, response_data) = {
            let mut executor = TurnExecutor::new(
                self.provider_state.config(),
                &self.runtime,
                &mut self.permission_state,
                &mut self.memory_state,
                ToolInfra {
                    closure_registry: self.tool_state.closure_registry(),
                    mcp_registry: self.mcp_state.mcp_registry(),
                    tool_server_handle: self.mcp_state.mcp_tool_server_handle().clone(),
                    visible_tool_definitions,
                },
            );

            let outcome = executor.execute(
                ui,
                ExecuteInput {
                    prompt,
                    preamble,
                    span,
                },
                &cached_client,
                &self.engine,
                self.final_session_id.as_deref(),
            );

            // Extract response data before executor is dropped
            let response_data = executor.take_response_data();
            (outcome, response_data)
        };

        // Restore the client immediately after the executor completes.
        self.provider_state.restore_client(cached_client);

        match outcome? {
            TurnOutcome::EarlyReturn(value) => Ok(value),
            TurnOutcome::Completed => {
                // Evaluate auto-compaction (runtime method) after turn completes
                let mut compaction_count = 0;
                if self.final_session_id.is_some() {
                    if let Some(CompactionTriggerDecision::Fire { source, .. }) =
                        self.evaluate_auto_compaction()
                        && let Err(error) = self.execute_compaction_event(ui, source)
                    {
                        ui.emit(&UiEvent::Warning { message: error });
                    }
                    compaction_count = self.compaction_state.compaction_count();
                }

                Ok(build_response(
                    response_data,
                    self.provider_state.config(),
                    self.final_session_id.as_deref(),
                    compaction_count,
                    span,
                ))
            }
        }
    }
}

impl ExtendedRuntime for AgentConversationRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        self.mcp_state.set_mcp_server_enabled(
            server_name,
            enabled,
            &self.runtime,
            self.tool_state.tool_definitions_mut(),
        )
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.mcp_state
            .llm_visible_mcp_tool_count(&self.tool_state.active_definitions(
                self.mcp_state.mcp_registry(),
                self.permission_state.permissions(),
            ))
    }

    fn llm_visible_mcp_tool_count_for_server(&self, server_name: &str) -> usize {
        self.mcp_state.llm_visible_mcp_tool_count_for_server(
            server_name,
            &self.tool_state.active_definitions(
                self.mcp_state.mcp_registry(),
                self.permission_state.permissions(),
            ),
        )
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        self.mcp_state
            .llm_visible_mcp_tool_names_by_server(&self.tool_state.active_definitions(
                self.mcp_state.mcp_registry(),
                self.permission_state.permissions(),
            ))
    }

    fn switch_model(&mut self, model_spec: &str) -> Result<String, String> {
        self.provider_state.switch_model(model_spec)
    }

    fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
        let cwd = self
            .mcp_state
            .mcp_caller_cwd()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "agent switch unavailable: working directory not set".to_string())?;

        let result = self.persona_state.switch_agent(
            agent_name,
            &cwd,
            self.multi_agent_state.agents_config(),
        )?;

        // If persona specifies a model, attempt to switch (ignore errors)
        if let Some(ref model) = result.model {
            let _ = self.switch_model(model);
        }

        // Reset tool definitions to baseline on agent switch
        self.tool_state.reset_to_baseline();

        // Invalidate cached client to pick up any changes
        self.provider_state.invalidate_cache();

        Ok(result.identity)
    }

    fn active_model_identity(&self) -> String {
        self.persona_state
            .active_model_identity(self.provider_state.config())
    }

    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        self.compaction_state
            .evaluate_auto_compaction(self.memory_state.last_total_tokens())
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        self.execute_compaction_event(ui, source)
    }

    fn clear_session(&mut self) {
        self.memory_state.clear();
    }
}

impl AgentConversationRuntime {
    // ── Phase I accessor methods ────────────────────────────────────

    /// Spawn a future on the tokio runtime.
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }

    pub fn provider(&self) -> &str {
        &self.provider_state.config().provider
    }

    pub fn model(&self) -> &str {
        &self.provider_state.config().model
    }

    pub fn max_context_tokens(&self) -> Option<u64> {
        self.provider_state
            .config()
            .max_context_tokens
            .map(u64::from)
    }

    pub fn startup_plugin_config(&self) -> Option<&crate::config::PluginConfig> {
        self.provider_state.startup_plugin_config()
    }

    pub fn agent_identity(&self) -> Option<&str> {
        self.persona_state.agent_identity()
    }

    pub fn mcp_caller_cwd(&self) -> Option<&std::path::Path> {
        self.mcp_state.mcp_caller_cwd()
    }

    pub fn mcp_lifecycle_projection(&self) -> &[crate::tools::mcp::runtime::McpServerLifecycle] {
        self.mcp_state.mcp_lifecycle_projection()
    }

    pub fn available_agent_summaries(&self) -> &[crate::protocol::persona::PersonaSummary] {
        self.multi_agent_state.available_agent_summaries()
    }

    pub fn take_mailbox_rx(
        &mut self,
    ) -> Option<std::sync::mpsc::Receiver<crate::mailbox::IncomingMessage>> {
        self.multi_agent_state.take_mailbox_rx()
    }

    // ── End Phase I accessor methods ────────────────────────────────

    fn ensure_memory_hydrated(&mut self) -> Result<(), LabeledError> {
        let mut count = self.compaction_state.compaction_count();
        let result = self.memory_state.ensure_memory_hydrated(
            self.final_session_id.as_deref(),
            &self.runtime,
            &mut count,
        );
        self.compaction_state.set_compaction_count(count);
        result
    }

    fn execute_compaction_event<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        if self
            .compaction_state
            .compacting()
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Ok(());
        }
        let _guard = CompactionGuard(Arc::clone(self.compaction_state.compacting()));

        self.provider_state
            .ensure_client_cached()
            .map_err(|e| e.to_string())?;
        self.ensure_memory_hydrated().map_err(|e| e.to_string())?;

        let session_id = self
            .final_session_id
            .as_ref()
            .ok_or_else(|| "session_unavailable".to_string())?;

        let result = CompactionExecutor::new(
            self.provider_state.config(),
            &self.runtime,
            self.memory_state.memory(),
            self.memory_state.conversation_store(),
            &self.store,
            self.memory_state.last_total_tokens(),
            session_id,
        )
        .execute(ui, source, self.provider_state.client().unwrap())?;

        if let Some(new_count) = result {
            self.compaction_state.set_compaction_count(new_count);
            // Reset so stale pre-compaction token count can't re-trigger
            *self.memory_state.last_total_tokens_mut() = None;
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
