use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};
use rig::client::CompletionClient;
use rig::memory::ConversationMemory;

use crate::{
    compaction::{CompactionInvocationMode, CompactionStrategy},
    config::Config,
    plugin::RuntimeCtx,
    session::{
        ConversationStore,
        JsonlConversationStore, SessionStore, StoreEntry, extract_llm_context,
    },
    tools::{closure::ClosureRegistry, executor::ToolExecutor},
};

use crate::agent::{
    protocol::{
        compaction::{
            CompactionTriggerDecision, CompactionTriggerPolicy, CompactionTriggerSource,
            TokenCompactionPolicy,
        },
        contracts::{ConversationRuntime, McpUsabilityState, ProgressUi},
        event::UiEvent,
    },
    tools::{
        authz::{AsyncAskHook, PermissionsConfig, SessionGrantCache},
        handler::{self, McpToolRegistry},
    },
};
use crate::tools::mcp::{
    config::McpServerConfig,
    runtime::{McpRuntime, McpServerLifecycle},
};
use super::compaction::{
    COMPACTION_FAILURE_WARNING, CompactionGuard, CompactionInvocation,
    execute_compaction, execute_compaction_event_shared,
};
use super::providers::{
    build_anthropic_client, build_copilot_client, build_ollama_client, build_openai_client,
    resolve_provider_type, CachedProviderClient, ClientCacheKey,
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
    #[allow(dead_code)]
    pub runtime_ctx: RuntimeCtx,
    pub config: Config,
    pub tool_definitions: Vec<rig::completion::ToolDefinition>,
    pub baseline_tool_definitions: Vec<rig::completion::ToolDefinition>,
    pub closure_registry: ClosureRegistry,
    pub mcp_registry: McpToolRegistry,
    pub mcp_runtime: Option<McpRuntime>,
    pub mcp_tool_server_handle: rig::tool::server::ToolServerHandle,
    pub mcp_lifecycle_projection: Vec<McpServerLifecycle>,
    pub mcp_server_configs: Vec<McpServerConfig>,
    pub mcp_caller_cwd: Option<std::path::PathBuf>,
    #[allow(dead_code)]
    pub tool_executor: ToolExecutor,
    pub engine: EngineInterface,
    pub store: SessionStore,
    pub final_session_id: Option<String>,
    pub context_window_max_tokens: u64,
    pub compaction_threshold_pct: f64,
    pub compaction_count: usize,
    pub compaction_strategy: CompactionStrategy,
    pub startup_plugin_config: Option<crate::config::PluginConfig>,
    pub permissions: PermissionsConfig,
    pub permissions_startup_summary: String,
    pub permissions_startup_emitted: bool,
    pub session_grants: SessionGrantCache,
    pub ask_hook: AsyncAskHook,
    pub memory: rig::memory::InMemoryConversationMemory,
    pub conversation_store: JsonlConversationStore,
    pub memory_hydrated: bool,
    pub cached_client: Option<CachedProviderClient>,
    pub cached_client_key: Option<ClientCacheKey>,
    #[allow(dead_code)]
    pub agent_persona_body: Option<String>,
    #[allow(dead_code)]
    pub agent_identity: Option<String>,
    #[allow(dead_code)]
    pub agent_description: Option<String>,
    #[allow(dead_code)]
    pub cached_agents_chain: Option<String>,
    #[allow(dead_code)]
    pub cached_available_skills: Option<String>,
    #[allow(dead_code)]
    pub cached_sub_agent_instruction: Option<String>,
    #[allow(dead_code)]
    pub orchestrator: Option<
        std::sync::Arc<
            std::sync::Mutex<crate::agent::tools::handler::spawn_agent::OrchestratorState>,
        >,
    >,
    #[allow(dead_code)]
    pub broker_sender:
        Option<std::sync::Arc<tokio::sync::Mutex<crate::agent::mailbox::BrokerSender>>>,
    #[allow(dead_code)]
    pub mailbox_rx: Option<std::sync::mpsc::Receiver<crate::agent::mailbox::IncomingMessage>>,
    #[allow(dead_code)]
    pub parent_name: Option<String>,
    pub compacting: Arc<AtomicBool>,
    pub last_total_tokens: Option<u64>,
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

use super::mcp_helpers::{
    mcp_enable_runtime_config, rebuild_mcp_lifecycle_projection, stage_enabled_mcp_runtime_state,
};

impl ConversationRuntime for AgentConversationRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        if !enabled {
            self.mcp_registry.set_server_enabled(server_name, false)?;
            self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                self.mcp_runtime.as_ref(),
                &self.mcp_server_configs,
                &self.mcp_registry,
                &self.tool_definitions,
            );
            return Ok(McpUsabilityState::Disabled);
        }

        if !self
            .mcp_server_configs
            .iter()
            .any(|server| server.name == server_name)
        {
            self.mcp_registry.set_server_enabled(server_name, false)?;
            self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                self.mcp_runtime.as_ref(),
                &self.mcp_server_configs,
                &self.mcp_registry,
                &self.tool_definitions,
            );
            return Ok(McpUsabilityState::Failed);
        }

        let runtime_config =
            mcp_enable_runtime_config(&self.mcp_server_configs, &self.mcp_registry, server_name);

        match self
            .runtime
            .block_on(crate::tools::mcp::runtime::connect_servers(
                &runtime_config,
                self.mcp_caller_cwd.as_deref(),
            )) {
            Ok(runtime) if runtime.has_sessions() => {
                let discovered = runtime.discovered_tools().to_vec();

                let (staged_tool_definitions, staged_registry) = stage_enabled_mcp_runtime_state(
                    &self.tool_definitions,
                    &self.mcp_registry,
                    server_name,
                    &discovered,
                )?;

                self.tool_definitions = staged_tool_definitions;
                self.mcp_registry = staged_registry;
                self.mcp_runtime = Some(runtime);
                self.mcp_tool_server_handle = self
                    .mcp_runtime
                    .as_ref()
                    .map(McpRuntime::tool_server_handle)
                    .unwrap_or_else(|| rig::tool::server::ToolServer::new().run());
                self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                    self.mcp_runtime.as_ref(),
                    &self.mcp_server_configs,
                    &self.mcp_registry,
                    &self.tool_definitions,
                );

                Ok(McpUsabilityState::Enabled)
            }
            Ok(_) | Err(_) => {
                self.mcp_registry.set_server_enabled(server_name, false)?;
                self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                    self.mcp_runtime.as_ref(),
                    &self.mcp_server_configs,
                    &self.mcp_registry,
                    &self.tool_definitions,
                );
                Ok(McpUsabilityState::Failed)
            }
        }
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.active_tool_definitions()
            .iter()
            .filter(|tool| self.mcp_registry.is_registered(tool.name.as_str()))
            .count()
    }

    fn llm_visible_mcp_tool_count_for_server(&self, server_name: &str) -> usize {
        self.active_tool_definitions()
            .iter()
            .filter(|tool| self.mcp_registry.is_registered(tool.name.as_str()))
            .filter_map(|tool| self.mcp_registry.server_name_for(tool.name.as_str()))
            .filter(|server| *server == server_name)
            .count()
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();

        for tool in self
            .active_tool_definitions()
            .iter()
            .filter(|tool| self.mcp_registry.is_registered(tool.name.as_str()))
        {
            let Some(server_name) = self.mcp_registry.server_name_for(tool.name.as_str()) else {
                continue;
            };
            grouped
                .entry(server_name.to_string())
                .or_default()
                .push(tool.name.clone());
        }

        grouped
            .into_iter()
            .map(|(server, mut names)| {
                names.sort();
                names.dedup();
                (server, names)
            })
            .collect()
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
        use crate::agent::protocol::persona::{
            FrontMatterParser, FsPersonaResolver, PersonaFileResolver,
            PulldownCmarkFrontMatterParser, interpret_front_matter,
        };

        let cwd = self
            .mcp_caller_cwd
            .clone()
            .ok_or_else(|| "agent switch unavailable: working directory not set".to_string())?;

        let config_dir = crate::utils::xdg::config_dir()
            .map(|base| base.join("nu-agent"))
            .map_err(|e| format!("agent switch failed: cannot determine config directory: {e}"))?;

        let resolver = FsPersonaResolver::new(cwd, config_dir, self.agents_config.clone());
        let (_path, contents) = resolver
            .resolve(agent_name)
            .map_err(|e| format!("agent switch failed: {e}"))?;

        let parser = PulldownCmarkFrontMatterParser;
        let raw = parser
            .parse(&contents)
            .map_err(|e| format!("agent switch failed: invalid front matter: {e}"))?;

        let parsed = interpret_front_matter(raw.front_matter.as_ref(), raw.body)
            .map_err(|e| format!("agent switch failed: invalid front matter fields: {e}"))?;

        // Update persona body
        self.agent_persona_body = Some(parsed.body);

        // Resolve identity: front matter name > agent_name argument
        let identity = parsed.name.unwrap_or_else(|| agent_name.to_string());
        self.agent_identity = Some(identity.clone());
        self.agent_description = parsed.description;

        // If persona specifies a model, attempt to switch (ignore errors)
        if let Some(ref model) = parsed.model {
            let _ = self.switch_model(model);
        }

        // Reset tool definitions to baseline on agent switch
        self.tool_definitions = self.baseline_tool_definitions.clone();

        // Invalidate cached client to pick up any changes
        self.cached_client = None;
        self.cached_client_key = None;

        log::debug!(
            "switch_agent: switched to identity={identity:?}, model={:?}, body_len={}",
            parsed.model,
            self.agent_persona_body.as_ref().map_or(0, |b| b.len())
        );

        Ok(identity)
    }

    fn active_model_identity(&self) -> String {
        format!("{}/{}", self.config.provider, self.config.model)
    }

    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        let policy = TokenCompactionPolicy::new(
            self.context_window_max_tokens,
            self.compaction_threshold_pct,
            self.compaction_strategy,
        );
        Some(policy.evaluate(self.last_total_tokens))
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        self.execute_compaction_event(ui, source)
    }

    fn clear_session(&mut self) {
        self.memory = rig::memory::InMemoryConversationMemory::new();
        self.memory_hydrated = false;
    }

    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        use crate::agent::conversation::turn::{TurnContext, execute_turn};
        use crate::agent::hook::AuthzPermissionResolver;

        emit_permissions_startup_summary_once(
            ui,
            &mut self.permissions_startup_emitted,
            &self.permissions_startup_summary,
        );

        // Build system preamble from cached components
        let preamble = build_system_preamble(
            self.config.preamble.as_deref(),
            self.agent_persona_body.as_deref(),
            self.cached_sub_agent_instruction.as_deref(),
            context.as_deref(),
            self.cached_agents_chain.as_deref(),
            self.cached_available_skills.as_deref(),
        );

        log::debug!(
            "execute_turn: preamble present={}, preamble_len={}",
            preamble.is_some(),
            preamble.as_ref().map_or(0, |p| p.len())
        );

        // Hydrate memory from conversation store (idempotent, guarded)
        self.ensure_memory_hydrated()?;

        let conversation_id = if let Some(ref session_id) = self.final_session_id {
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

        // Provider dispatch - build model from cached client
        self.ensure_client_cached()?;

        // Compute filtered tool definitions before entering the mutable borrow scope
        let visible_tool_definitions = self.active_tool_definitions();

        // Macro to reduce TurnContext boilerplate
        macro_rules! run_with_model {
            ($model:expr) => {{
                let mut permission_resolver = AuthzPermissionResolver {
                    permissions: &self.permissions,
                    grant_cache: &mut self.session_grants,
                    ask_hook: &mut self.ask_hook,
                    engine: &self.engine,
                    closure_registry: &self.closure_registry,
                    mcp_registry: &self.mcp_registry,
                };
                execute_turn(
                    TurnContext {
                        runtime: self.runtime.handle(),
                        model: $model,
                        prompt: prompt.clone(),
                        memory: self.memory.clone(),
                        conversation_id,
                        preamble: preamble.as_deref(),
                        max_turns: self.config.max_tool_turns,
                        tool_server_handle: self.mcp_tool_server_handle.clone(),
                        visible_tool_definitions: visible_tool_definitions.clone(),
                        closure_registry: &self.closure_registry,
                        mcp_registry: &self.mcp_registry,
                    },
                    ui,
                    &mut permission_resolver,
                )
            }};
        }

        macro_rules! with_cached_model {
            ($model_var:ident, $body:expr) => {
                match self.cached_client.as_ref().unwrap() {
                    CachedProviderClient::Copilot(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::OpenAi(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::Anthropic(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::Ollama(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                }
            };
        }

        let turn_result = match with_cached_model!(model, run_with_model!(model)) {
            Ok(result) => result,
            Err(e) if e.cancelled => {
                // Path A: rig hook cancelled — persist chat_history if available
                if let Some(ref session_id) = self.final_session_id
                    && let Some(ref messages) = e.messages
                {
                    if let Err(persist_err) = self.conversation_store.append(session_id, messages, None) {
                        log::warn!("Failed to persist cancelled turn messages: {}", persist_err);
                    }
                    if let Err(mem_err) = self
                        .runtime
                        .block_on(self.memory.append(session_id, messages.clone()))
                    {
                        log::warn!(
                            "Failed to update in-memory context for cancelled turn (path A): {}",
                            mem_err
                        );
                    }
                }
                // Return a minimal cancelled response (not an error)
                let llm_response = crate::llm::LlmResponse {
                    text: String::new(),
                    usage: crate::llm::LlmUsage {
                        input_tokens: 0,
                        output_tokens: 0,
                        total_tokens: 0,
                        cached_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    },
                    tool_calls: Vec::new(),
                    tool_call_metadata: Vec::new(),
                };
                return Ok(crate::llm::format_response(
                    &llm_response,
                    &self.config,
                    self.final_session_id.as_deref(),
                    self.compaction_count,
                    span,
                ));
            }
            Err(e) => {
                return Err(
                    LabeledError::new(format!("Turn failed: {}", e.msg)).with_label(e.msg, span)
                );
            }
        };

        // Path B: cancel_token fired — FinalResponse never arrived so messages is None.
        // Construct user + optional partial assistant message and persist manually.
        if turn_result.cancelled
            && turn_result.messages.is_none()
            && let Some(ref session_id) = self.final_session_id
        {
            use rig::completion::Message;
            let mut cancelled_messages = vec![Message::user(prompt.clone())];
            if !turn_result.text.is_empty() {
                cancelled_messages.push(Message::assistant(turn_result.text.clone()));
            }
            if let Err(e) = self
                .conversation_store
                .append(session_id, &cancelled_messages, None)
            {
                log::warn!("Failed to persist cancelled turn messages (path B): {}", e);
            }
            if let Err(e) = self
                .runtime
                .block_on(self.memory.append(session_id, cancelled_messages.clone()))
            {
                log::warn!(
                    "Failed to update in-memory context for cancelled turn (path B): {}",
                    e
                );
            }
        }

        // Persist new messages to conversation store if session exists
        if let Some(ref session_id) = self.final_session_id
            && let Some(ref messages) = turn_result.messages
        {
            // Persist the new messages from the turn result
            if let Err(e) = self.conversation_store.append(session_id, messages, Some(turn_result.last_total_tokens)) {
                log::warn!(
                    "Failed to persist turn messages to conversation store: {}",
                    e
                );
            }

            // Update last_total_tokens for compaction
            self.last_total_tokens = Some(turn_result.last_total_tokens);
        }

        // Format the response value
        let message_count = 0;
        let mut compaction_count = 0;

        if self.final_session_id.is_some() {
            if let Some(CompactionTriggerDecision::Fire { source, .. }) =
                self.evaluate_auto_compaction()
                && let Err(error) = self.execute_compaction_event(ui, source)
            {
                ui.emit(&UiEvent::Warning { message: error });
            }

            compaction_count = self.compaction_count;
        }

        // Only emit the full response if deltas weren't already emitted during streaming
        if !turn_result.deltas_emitted {
            ui.emit(&UiEvent::AssistantMessage {
                text: turn_result.text.clone(),
            });
        }
        ui.emit(&UiEvent::Completed {
            tool_calls: turn_result.tool_call_count,
        });
        ui.flush();

        // Build the response value with the same structure as the old path
        let llm_response = crate::llm::LlmResponse {
            text: turn_result.text,
            usage: crate::llm::LlmUsage {
                input_tokens: turn_result.usage.input_tokens,
                output_tokens: turn_result.usage.output_tokens,
                total_tokens: turn_result.usage.total_tokens,
                cached_input_tokens: turn_result.usage.cached_input_tokens,
                cache_creation_input_tokens: turn_result.usage.cache_creation_input_tokens,
            },
            tool_calls: Vec::new(), // TODO: track tool calls in TurnResult
            tool_call_metadata: Vec::new(), // TODO: track tool metadata in TurnResult
        };

        let response_value = crate::llm::format_response(
            &llm_response,
            &self.config,
            self.final_session_id.as_deref(),
            compaction_count,
            span,
        );

        if self.final_session_id.is_some()
            && let Ok(record) = response_value.as_record()
        {
            let mut new_record = record.clone();
            if let Some(meta_value) = new_record.get("_meta")
                && let Ok(meta_record) = meta_value.as_record()
            {
                let mut new_meta = meta_record.clone();
                new_meta.insert(
                    "message_count".to_string(),
                    Value::int(message_count as i64, span),
                );

                new_record.insert("_meta".to_string(), Value::record(new_meta, span));
                return Ok(Value::record(new_record, span));
            }
        }

        Ok(response_value)
    }
}

impl AgentConversationRuntime {
    /// Idempotent memory hydration: loads stored messages into in-memory
    /// conversation memory exactly once per runtime lifetime (or until
    /// `clear_session` resets the guard).
    ///
    /// This must be called before any operation that reads memory
    /// (`execute_turn`, `execute_compaction_event`) so that reloaded
    /// sessions have their history available.
    fn ensure_memory_hydrated(&mut self) -> Result<(), LabeledError> {
        if self.memory_hydrated {
            return Ok(());
        }
        if let Some(ref session_id) = self.final_session_id {
            // Load ALL entries (messages + markers)
            let (entries, last_total_tokens) = self
                .conversation_store
                .load_all(session_id)
                .map_err(|e| LabeledError::new(format!("Failed to load session entries: {}", e)))?;

            // Extract only LLM-relevant messages (from latest marker onward)
            let llm_context = extract_llm_context(&entries);

            if !llm_context.is_empty() {
                self.runtime
                    .block_on(self.memory.append(session_id, llm_context.clone()))
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to append messages to memory: {}", e))
                    })?;
            }
            self.last_total_tokens = last_total_tokens;

            // Derive compaction_count from markers
            let marker_count = entries
                .iter()
                .filter(|e| matches!(e, StoreEntry::Marker(_)))
                .count();
            self.compaction_count = marker_count;
        }
        self.memory_hydrated = true;
        Ok(())
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

    fn active_tool_definitions(&self) -> Vec<rig::completion::ToolDefinition> {
        handler::llm_visible_tool_definitions(
            &self.tool_definitions,
            &self.mcp_registry,
            &self.permissions,
        )
    }

    fn execute_compaction_event<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        // Prevent overlapping compactions: if another is already running, skip.
        if self
            .compacting
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Ok(());
        }
        let _guard = CompactionGuard(Arc::clone(&self.compacting));

        self.ensure_client_cached().map_err(|e| e.to_string())?;

        // Ensure memory is hydrated before compaction reads it
        self.ensure_memory_hydrated().map_err(|e| e.to_string())?;

        let runtime = &self.runtime;
        let memory = &self.memory;
        let conversation_store = &self.conversation_store;
        let store = &self.store;
        let last_total_tokens = self.last_total_tokens;

        let source_label = source.as_str().to_string();
        ui.emit(&UiEvent::CompactionStarted {
            source: source_label.clone(),
        });

        // Load session temporarily for compaction
        let session_id = self
            .final_session_id
            .as_ref()
            .ok_or_else(|| "session_unavailable".to_string())?;

        let mut session = store
            .load_session(session_id)
            .map_err(|e| format!("Failed to load session for compaction: {}", e))?;

        // Macro to reduce boilerplate for compaction
        macro_rules! compact_with_model {
            ($model:expr) => {{
                execute_compaction_event_shared(source, || {
                    let mode = match source {
                        CompactionTriggerSource::SlashCompact => CompactionInvocationMode::Force,
                        CompactionTriggerSource::AutoThreshold => {
                            CompactionInvocationMode::Threshold
                        }
                    };
                    runtime.block_on(execute_compaction(
                        &mut session,
                        memory,
                        conversation_store,
                        $model.clone(),
                        ui,
                        CompactionInvocation {
                            mode,
                            source: &source_label,
                            last_total_tokens,
                        },
                    ))
                })
            }};
        }

        macro_rules! with_cached_model {
            ($model_var:ident, $body:expr) => {
                match self.cached_client.as_ref().unwrap() {
                    CachedProviderClient::Copilot(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::OpenAi(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::Anthropic(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::Ollama(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                }
            };
        }

        let result = with_cached_model!(model, compact_with_model!(model));

        match result {
            Ok(event) => {
                ui.emit(&event);

                // Update compaction_count after successful compaction
                if let UiEvent::CompactionTriggered { .. } = &event {
                    self.compaction_count = session.compaction_count();
                    // Reset so stale pre-compaction token count can't re-trigger
                    self.last_total_tokens = None;
                }

                Ok(())
            }
            Err(error) => {
                ui.emit(&UiEvent::CompactionFailed {
                    source: source_label,
                    message: COMPACTION_FAILURE_WARNING.to_string(),
                });
                Err(error)
            }
        }
    }
}

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
