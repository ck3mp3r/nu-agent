use std::sync::Arc;
use std::sync::atomic::Ordering;

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};

use crate::types::{InMemoryConversationMemory, ToolDefinition};
use crate::{config::Config, session::SessionStore, tools::closure::ClosureRegistry};

use super::compaction::CompactionGuard;
use super::compaction_executor::CompactionExecutor;
use super::providers::{
    CachedProviderClient, ClientCacheKey, build_anthropic_client, build_copilot_client,
    build_ollama_client, build_openai_client, resolve_provider_type,
};
use crate::agent::{
    protocol::{
        compaction::{CompactionTriggerDecision, CompactionTriggerSource},
        contracts::{CoreRuntime, ExtendedRuntime, McpUsabilityState, ProgressUi},
        event::UiEvent,
    },
    tools::{
        authz::{AsyncAskHook, PermissionsConfig, SessionGrantCache},
        handler,
    },
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

pub(crate) struct AgentConversationRuntime {
    pub runtime: tokio::runtime::Runtime,
    pub config: Config,
    pub tool_definitions: Vec<ToolDefinition>,
    pub baseline_tool_definitions: Vec<ToolDefinition>,
    pub closure_registry: ClosureRegistry,
    pub mcp_state: super::mcp_state::McpState,
    pub engine: EngineInterface,
    pub store: SessionStore,
    pub final_session_id: Option<String>,
    pub compaction_state: super::compaction_state::CompactionState,
    pub startup_plugin_config: Option<crate::config::PluginConfig>,
    pub permissions: PermissionsConfig,
    pub permissions_startup_summary: String,
    pub permissions_startup_emitted: bool,
    pub session_grants: SessionGrantCache,
    pub ask_hook: AsyncAskHook,
    pub memory_state: super::memory_state::MemoryState,
    pub cached_client: Option<CachedProviderClient>,
    pub cached_client_key: Option<ClientCacheKey>,
    pub persona_state: super::persona_state::PersonaState,
    pub mailbox_rx: Option<std::sync::mpsc::Receiver<crate::agent::mailbox::IncomingMessage>>,
    pub available_agent_summaries: Vec<crate::agent::protocol::persona::PersonaSummary>,
    pub agents_config: crate::config::AgentsConfig,
}

fn emit_permissions_startup_summary_once<U: ProgressUi>(
    ui: &mut U,
    emitted: &mut bool,
    summary: &str,
) {
    if !*emitted {
        ui.emit(&UiEvent::Warning {
            message: summary.to_string(),
        });
        *emitted = true;
    }
}

fn apply_switched_config(current: &mut Config, switched: Config) {
    *current = switched;
}

impl CoreRuntime for AgentConversationRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        use super::turn_executor::{
            ConversationState, ExecuteInput, PermissionCtx, ToolInfra, TurnExecutor, TurnOutcome,
            build_response,
        };

        emit_permissions_startup_summary_once(
            ui,
            &mut self.permissions_startup_emitted,
            &self.permissions_startup_summary,
        );

        // Build system preamble from cached components
        let preamble = build_system_preamble(
            self.config.preamble.as_deref(),
            self.persona_state.agent_persona_body.as_deref(),
            self.persona_state.cached_sub_agent_instruction.as_deref(),
            context.as_deref(),
            self.persona_state.cached_agents_chain.as_deref(),
            self.persona_state.cached_available_skills.as_deref(),
        );

        log::debug!(
            "execute_turn: preamble present={}, preamble_len={}",
            preamble.is_some(),
            preamble.as_ref().map_or(0, |p| p.len())
        );

        // Hydrate memory from conversation store (idempotent, guarded)
        self.ensure_memory_hydrated()?;

        // Provider dispatch - build model from cached client
        self.ensure_client_cached()?;

        // Compute filtered tool definitions before entering the mutable borrow scope
        let visible_tool_definitions = self.active_tool_definitions();

        // Take the client temporarily to avoid overlapping borrows with `self`.
        let cached_client = self.cached_client.take().unwrap();

        // Scope the executor so its borrows are released before compaction.
        let (outcome, response_data) = {
            let mut executor = TurnExecutor::new(
                &self.config,
                &self.runtime,
                PermissionCtx {
                    permissions: &self.permissions,
                    session_grants: &mut self.session_grants,
                    ask_hook: &mut self.ask_hook,
                },
                ConversationState {
                    memory: &mut self.memory_state.memory,
                    conversation_store: &self.memory_state.conversation_store,
                    last_total_tokens: &mut self.memory_state.last_total_tokens,
                    final_session_id: &self.final_session_id,
                },
                ToolInfra {
                    closure_registry: &self.closure_registry,
                    mcp_registry: &self.mcp_state.mcp_registry,
                    mcp_tool_server_handle: &self.mcp_state.mcp_tool_server_handle,
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
                visible_tool_definitions,
                &self.engine,
            );

            // Extract response data before executor is dropped
            let response_data = executor.take_response_data();
            (outcome, response_data)
        };

        // Restore the client immediately after the executor completes.
        self.cached_client = Some(cached_client);

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
                    compaction_count = self.compaction_state.compaction_count;
                }

                Ok(build_response(
                    response_data,
                    &self.config,
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
            &mut self.tool_definitions,
        )
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.mcp_state
            .llm_visible_mcp_tool_count(&self.active_tool_definitions())
    }

    fn llm_visible_mcp_tool_count_for_server(&self, server_name: &str) -> usize {
        self.mcp_state
            .llm_visible_mcp_tool_count_for_server(server_name, &self.active_tool_definitions())
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        self.mcp_state
            .llm_visible_mcp_tool_names_by_server(&self.active_tool_definitions())
    }

    fn switch_model(&mut self, model_spec: &str) -> Result<String, String> {
        let plugin_config = self.startup_plugin_config.clone().ok_or_else(|| {
            "model switch unavailable: startup plugin config cache is missing".to_string()
        })?;

        let resolved = plugin_config.resolve_model(model_spec)?;
        apply_switched_config(&mut self.config, resolved);
        self.cached_client = None;
        self.cached_client_key = None;
        Ok(format!("{}/{}", self.config.provider, self.config.model))
    }

    fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
        let cwd = self
            .mcp_state
            .mcp_caller_cwd
            .clone()
            .ok_or_else(|| "agent switch unavailable: working directory not set".to_string())?;

        let result = self
            .persona_state
            .switch_agent(agent_name, &cwd, &self.agents_config)?;

        // If persona specifies a model, attempt to switch (ignore errors)
        if let Some(ref model) = result.model {
            let _ = self.switch_model(model);
        }

        // Reset tool definitions to baseline on agent switch
        self.tool_definitions = self.baseline_tool_definitions.clone();

        // Invalidate cached client to pick up any changes
        self.cached_client = None;
        self.cached_client_key = None;

        Ok(result.identity)
    }

    fn active_model_identity(&self) -> String {
        self.persona_state.active_model_identity(&self.config)
    }

    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        self.compaction_state
            .evaluate_auto_compaction(self.memory_state.last_total_tokens)
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        self.execute_compaction_event(ui, source)
    }

    fn clear_session(&mut self) {
        self.memory_state.memory = InMemoryConversationMemory::new();
        self.memory_state.memory_hydrated = false;
    }
}

impl AgentConversationRuntime {
    fn ensure_memory_hydrated(&mut self) -> Result<(), LabeledError> {
        self.memory_state.ensure_memory_hydrated(
            self.final_session_id.as_deref(),
            &self.runtime,
            &mut self.compaction_state.compaction_count,
        )
    }

    fn client_cache_key(&self) -> ClientCacheKey {
        (
            self.config.provider.clone(),
            self.config.api_key.clone(),
            self.config.base_url.clone(),
        )
    }

    fn ensure_client_cached(&mut self) -> Result<(), LabeledError> {
        let key = self.client_cache_key();
        if self.cached_client_key.as_ref() == Some(&key) {
            return Ok(());
        }
        let provider_key = self.config.provider.as_str();
        let provider_type =
            resolve_provider_type(provider_key, self.config.provider_impl.as_deref());
        log::info!(
            "creating {} client (type={}) for model={}",
            provider_key,
            provider_type,
            self.config.model
        );
        let client = match provider_type {
            "copilot" | "github-copilot" | "github_copilot" => {
                CachedProviderClient::Copilot(build_copilot_client(&self.config)?)
            }
            "openai" => CachedProviderClient::OpenAi(build_openai_client(&self.config)?),
            "anthropic" => CachedProviderClient::Anthropic(build_anthropic_client(&self.config)?),
            "ollama" => CachedProviderClient::Ollama(build_ollama_client(&self.config)?),
            other => {
                return Err(LabeledError::new(format!(
                    "Unsupported provider: '{}' (from config key '{}')",
                    other, provider_key
                ))
                .with_help(
                    "Supported: copilot, openai, anthropic, ollama. \
                         Set 'provider' field in provider config to map custom names.",
                ));
            }
        };
        self.cached_client = Some(client);
        self.cached_client_key = Some(key);
        Ok(())
    }

    fn active_tool_definitions(&self) -> Vec<ToolDefinition> {
        handler::llm_visible_tool_definitions(
            &self.tool_definitions,
            &self.mcp_state.mcp_registry,
            &self.permissions,
        )
    }

    fn execute_compaction_event<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        if self
            .compaction_state
            .compacting
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Ok(());
        }
        let _guard = CompactionGuard(Arc::clone(&self.compaction_state.compacting));

        self.ensure_client_cached().map_err(|e| e.to_string())?;
        self.ensure_memory_hydrated().map_err(|e| e.to_string())?;

        let session_id = self
            .final_session_id
            .as_ref()
            .ok_or_else(|| "session_unavailable".to_string())?;

        let result = CompactionExecutor::new(
            &self.config,
            &self.runtime,
            &self.memory_state.memory,
            &self.memory_state.conversation_store,
            &self.store,
            self.memory_state.last_total_tokens,
            session_id,
        )
        .execute(ui, source, self.cached_client.as_ref().unwrap())?;

        if let Some(new_count) = result {
            self.compaction_state.compaction_count = new_count;
            // Reset so stale pre-compaction token count can't re-trigger
            self.memory_state.last_total_tokens = None;
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
