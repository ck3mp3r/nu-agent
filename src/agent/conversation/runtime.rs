use std::time::Duration;

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};
use rig::memory::ConversationMemory;

use crate::{
    config::Config,
    plugin::RuntimeCtx,
    session::{
        CompactionInvocationMode, CompactionOutcome, ConversationStore, JsonlConversationStore,
        Session, SessionStore,
    },
    tools::{closure::ClosureRegistry, executor::ToolExecutor},
};

use crate::agent::{
    protocol::{
        compaction::{
            CompactionTriggerDecision, CompactionTriggerPolicy, CompactionTriggerSource,
            CompactionTriggerState, ThresholdCompactionPolicy,
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

const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

/// Build system preamble from components.
/// Joins non-empty parts with separators. Returns None if all empty.
fn build_system_preamble(
    config_preamble: Option<&str>,
    context: Option<&str>,
    agents_chain: Option<&str>,
    available_skills: Option<&str>,
) -> Option<String> {
    let parts: Vec<&str> = [config_preamble, context, agents_chain, available_skills]
        .into_iter()
        .flatten()
        .collect();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n---\n\n"))
    }
}

pub(crate) struct AgentConversationRuntime {
    pub runtime: tokio::runtime::Runtime,
    #[allow(dead_code)]
    pub runtime_ctx: RuntimeCtx,
    pub config: Config,
    pub tool_definitions: Vec<rig::completion::ToolDefinition>,
    pub closure_registry: ClosureRegistry,
    pub mcp_registry: McpToolRegistry,
    pub mcp_runtime: Option<McpRuntime>,
    pub mcp_tool_server_handle: rig::tool::server::ToolServerHandle,
    pub mcp_lifecycle_projection: Vec<McpServerLifecycle>,
    pub mcp_server_configs: Vec<McpServerConfig>,
    pub mcp_cli_patterns: Vec<String>,
    pub mcp_caller_cwd: Option<std::path::PathBuf>,
    #[allow(dead_code)]
    pub tool_executor: ToolExecutor,
    pub engine: EngineInterface,
    pub store: SessionStore,
    pub final_session_id: Option<String>,
    pub compaction_threshold: Option<usize>,
    pub compaction_count: usize,
    pub auto_compaction_tolerance: usize,
    pub auto_compaction_hysteresis_margin: usize,
    pub auto_compaction_state: CompactionTriggerState,
    pub startup_plugin_config: Option<crate::config::PluginConfig>,
    pub permissions: PermissionsConfig,
    pub permissions_startup_summary: String,
    pub permissions_startup_emitted: bool,
    pub session_grants: SessionGrantCache,
    pub ask_hook: AsyncAskHook,
    pub memory: rig::memory::InMemoryConversationMemory,
    pub conversation_store: JsonlConversationStore,
    pub memory_message_count: usize,
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

fn mcp_tool_definition_from_discovered(
    tool: &crate::tools::mcp::client::McpToolDefinition,
) -> rig::completion::ToolDefinition {
    rig::completion::ToolDefinition {
        name: tool.name.clone(),
        description: tool
            .description
            .clone()
            .unwrap_or_else(|| format!("MCP tool from server '{}'", tool.server)),
        parameters: tool.parameters.clone().unwrap_or_else(|| {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "args": {
                        "type": "array",
                        "items": {}
                    }
                },
                "required": ["args"]
            })
        }),
    }
}

fn merge_new_mcp_tools_into_runtime(
    tool_definitions: &mut Vec<rig::completion::ToolDefinition>,
    mcp_registry: &mut McpToolRegistry,
    discovered_tools: &[crate::tools::mcp::client::McpToolDefinition],
    cli_patterns: &[String],
) -> Result<(), String> {
    let filtered =
        crate::tools::mcp::registration::registerable_tools(discovered_tools, cli_patterns);
    if filtered.is_empty() {
        return Ok(());
    }

    mcp_registry.register_tools(filtered.clone())?;

    let known_names = tool_definitions
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<std::collections::HashSet<_>>();

    for tool in filtered {
        if !known_names.contains(tool.name.as_str()) {
            tool_definitions.push(mcp_tool_definition_from_discovered(&tool));
        }
    }

    Ok(())
}

fn stage_enabled_mcp_runtime_state(
    current_tool_definitions: &[rig::completion::ToolDefinition],
    current_registry: &McpToolRegistry,
    server_name: &str,
    discovered_tools: &[crate::tools::mcp::client::McpToolDefinition],
    cli_patterns: &[String],
) -> Result<(Vec<rig::completion::ToolDefinition>, McpToolRegistry), String> {
    let mut staged_tool_definitions = current_tool_definitions.to_vec();
    let mut staged_registry = current_registry.clone();

    merge_new_mcp_tools_into_runtime(
        &mut staged_tool_definitions,
        &mut staged_registry,
        discovered_tools,
        cli_patterns,
    )?;
    staged_registry.set_server_enabled(server_name, true)?;

    Ok((staged_tool_definitions, staged_registry))
}

fn mcp_enable_runtime_config(
    mcp_server_configs: &[McpServerConfig],
    mcp_registry: &McpToolRegistry,
    server_to_enable: &str,
) -> Vec<McpServerConfig> {
    mcp_server_configs
        .iter()
        .map(|server| {
            let enable =
                server.name == server_to_enable || mcp_registry.is_server_enabled(&server.name);
            McpServerConfig {
                enabled: enable,
                ..server.clone()
            }
        })
        .collect()
}

fn rebuild_mcp_lifecycle_projection(
    mcp_runtime: Option<&McpRuntime>,
    mcp_server_configs: &[McpServerConfig],
    mcp_registry: &McpToolRegistry,
    tool_definitions: &[rig::completion::ToolDefinition],
) -> Vec<McpServerLifecycle> {
    let visible_count_by_server = tool_definitions
        .iter()
        .filter(|tool| mcp_registry.contains(tool.name.as_str()))
        .filter_map(|tool| {
            mcp_registry
                .server_name_for(tool.name.as_str())
                .map(str::to_string)
        })
        .fold(
            std::collections::HashMap::<String, usize>::new(),
            |mut acc, server| {
                *acc.entry(server).or_insert(0) += 1;
                acc
            },
        );

    let projected_runtime_config: Vec<McpServerConfig> = mcp_server_configs
        .iter()
        .map(|server| McpServerConfig {
            enabled: mcp_registry.is_server_enabled(&server.name),
            ..server.clone()
        })
        .collect();

    if let Some(runtime) = mcp_runtime {
        runtime
            .lifecycle_projection(&projected_runtime_config)
            .into_iter()
            .map(|mut lifecycle| {
                lifecycle.visible_tool_count = visible_count_by_server
                    .get(lifecycle.name.as_str())
                    .copied()
                    .unwrap_or(0);
                lifecycle
            })
            .collect()
    } else {
        let mut projection = projected_runtime_config
            .iter()
            .map(|server| McpServerLifecycle {
                name: server.name.clone(),
                configured: true,
                enabled: server.enabled,
                connected: false,
                visible_tool_count: visible_count_by_server
                    .get(server.name.as_str())
                    .copied()
                    .unwrap_or(0),
            })
            .collect::<Vec<_>>();
        projection.sort_by(|a, b| a.name.cmp(&b.name));
        projection
    }
}

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
                    &self.mcp_cli_patterns,
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
        Ok(format!("{}/{}", self.config.provider, self.config.model))
    }

    fn active_model_identity(&self) -> String {
        format!("{}/{}", self.config.provider, self.config.model)
    }

    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        let Some(threshold) = self.compaction_threshold else {
            return Some(CompactionTriggerDecision::NoFire {
                reason: "signal_unavailable".to_string(),
            });
        };

        let policy = ThresholdCompactionPolicy::new(
            threshold,
            self.auto_compaction_tolerance,
            self.auto_compaction_hysteresis_margin,
        );
        Some(policy.evaluate(
            Some(self.memory_message_count),
            &mut self.auto_compaction_state,
        ))
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        self.execute_compaction_event(ui, source)
    }

    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        use crate::agent::conversation::turn::{TurnContext, TurnError, execute_turn};
        use crate::agent::hook::AuthzPermissionResolver;

        emit_permissions_startup_summary_once(
            ui,
            &mut self.permissions_startup_emitted,
            &self.permissions_startup_summary,
        );

        let loaded_agents = self
            .mcp_caller_cwd
            .as_deref()
            .map(crate::agent::protocol::agents::load_agents_chain_for_cwd)
            .unwrap_or_default();

        for warning in &loaded_agents.warnings {
            ui.emit(&UiEvent::Warning {
                message: warning.clone(),
            });
        }

        let available_skills = self
            .mcp_caller_cwd
            .as_deref()
            .and_then(crate::agent::protocol::skills::render_available_skills_preamble);

        // Build system preamble from components
        let preamble = build_system_preamble(
            self.config.preamble.as_deref(),
            context.as_deref(),
            loaded_agents.merged_chain.as_deref(),
            available_skills.as_deref(),
        );

        // Hydrate memory from conversation store on session attach
        let conversation_id = if let Some(ref session_id) = self.final_session_id {
            // Session exists: load from conversation store and populate memory
            let messages = self
                .conversation_store
                .load(session_id)
                .unwrap_or_else(|e| {
                    // Log error but continue with empty history
                    eprintln!("Failed to load conversation history: {}", e);
                    Vec::new()
                });

            // Append loaded messages to memory (memory.append is async and takes a Vec)
            if !messages.is_empty()
                && let Err(e) = self
                    .runtime
                    .block_on(self.memory.append(session_id, messages.clone()))
            {
                eprintln!("Failed to append messages to memory: {}", e);
            }

            // Track message count
            self.memory_message_count = messages.len();

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

        // Create the GitHub Copilot client with inline rig builder
        // Use turbofish syntax for api_key (required by CopilotAuth trait)
        let key = self.config.api_key.clone().ok_or_else(|| {
            LabeledError::new("Missing API key")
                .with_label(
                    "Set GITHUB_COPILOT_API_KEY or GITHUB_TOKEN, or provide api_key in config",
                    span,
                )
        })?;
        
        let builder = rig::providers::copilot::Client::builder()
            .api_key::<rig::providers::copilot::CopilotAuth>(key);
        
        let builder = if let Some(url) = &self.config.base_url {
            builder.base_url(url.clone())
        } else {
            builder
        };
        
        let client = builder.build().map_err(|e| {
            LabeledError::new(format!("Failed to create client: {}", e))
                .with_label(format!("{}", e), span)
        })?;

        // Create the real permission resolver using the authorization context
        let mut permission_resolver = AuthzPermissionResolver {
            permissions: &self.permissions,
            grant_cache: &mut self.session_grants,
            ask_hook: &mut self.ask_hook,
            engine: &self.engine,
            closure_registry: &self.closure_registry,
            mcp_registry: &self.mcp_registry,
        };

        // Call the new execute_turn function
        let turn_result = execute_turn(
            TurnContext {
                runtime: self.runtime.handle(),
                client: &client,
                model_name: &self.config.model,
                prompt,
                memory: self.memory.clone(),
                conversation_id,
                preamble: preamble.as_deref(),
                max_turns: self.config.max_tool_turns,
                tool_server_handle: self.mcp_tool_server_handle.clone(),
                closure_registry: &self.closure_registry,
                mcp_registry: &self.mcp_registry,
            },
            ui,
            &mut permission_resolver,
        )
        .map_err(|e: TurnError| {
            if e.cancelled {
                LabeledError::new(format!("Turn cancelled: {}", e.msg)).with_label(e.msg, span)
            } else {
                LabeledError::new(format!("Turn failed: {}", e.msg)).with_label(e.msg, span)
            }
        })?;

        // Persist new messages to conversation store if session exists
        if let Some(ref session_id) = self.final_session_id
            && let Some(ref messages) = turn_result.messages
        {
            // Persist the new messages from the turn result
            if let Err(e) = self.conversation_store.append(session_id, messages) {
                eprintln!(
                    "Warning: Failed to persist turn messages to conversation store: {}",
                    e
                );
            }

            // Update memory message count
            self.memory_message_count += messages.len();
        }

        // Format the response value
        let mut message_count = 0;
        let mut compaction_count = 0;

        if self.final_session_id.is_some() {
            if let Some(CompactionTriggerDecision::Fire { source, .. }) =
                self.evaluate_auto_compaction()
                && let Err(error) = self.execute_compaction_event(ui, source)
            {
                ui.emit(&UiEvent::Warning { message: error });
            }

            message_count = self.memory_message_count;
            compaction_count = self.compaction_count;
        }

        ui.emit(&UiEvent::AssistantMessage {
            text: turn_result.text.clone(),
        });
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
    fn active_tool_definitions(&self) -> Vec<rig::completion::ToolDefinition> {
        handler::llm_visible_tool_definitions(&self.tool_definitions, &self.mcp_registry)
    }

    fn execute_compaction_event<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        let runtime = &self.runtime;
        let memory = &self.memory;
        let conversation_store = &self.conversation_store;
        let store = &self.store;

        // Create the GitHub Copilot client with inline rig builder
        // Use turbofish syntax for api_key (required by CopilotAuth trait)
        let key = self.config.api_key.clone().ok_or_else(|| {
            "Missing API key - set GITHUB_COPILOT_API_KEY or GITHUB_TOKEN, or provide api_key in config".to_string()
        })?;
        
        let builder = rig::providers::copilot::Client::builder()
            .api_key::<rig::providers::copilot::CopilotAuth>(key);
        
        let builder = if let Some(url) = &self.config.base_url {
            builder.base_url(url.clone())
        } else {
            builder
        };
        
        let client = builder
            .build()
            .map_err(|e| format!("Failed to create client for compaction: {}", e))?;

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

        // Execute compaction with rig memory (async)
        let result = execute_compaction_event_shared(source, || {
            let mode = match source {
                CompactionTriggerSource::SlashCompact => CompactionInvocationMode::Force,
                CompactionTriggerSource::AutoThreshold => CompactionInvocationMode::Threshold,
            };

            // Run async compaction in the runtime
            runtime.block_on(execute_compaction(
                &mut session,
                memory,
                conversation_store,
                &client,
                &self.config.model,
                mode,
                ui,
            ))
        });

        match result {
            Ok(event) => {
                ui.emit(&event);

                // Update memory_message_count and compaction_count after successful compaction
                if let UiEvent::CompactionTriggered {
                    kept_recent_count, ..
                } = &event
                {
                    // After compaction: summary + kept_recent_count messages
                    self.memory_message_count = kept_recent_count + 1;
                    // Increment compaction count
                    self.compaction_count = session.compaction_count();
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

fn execute_compaction_event_shared<F>(
    source: CompactionTriggerSource,
    mut execute: F,
) -> Result<UiEvent, String>
where
    F: FnMut() -> Result<Option<CompactionOutcome>, String>,
{
    let outcome = execute()?;
    let (summarized_count, kept_recent_count, summary_body) = match outcome {
        Some(outcome) => (
            outcome.summarized_count,
            outcome.kept_recent_count,
            outcome.summary_text,
        ),
        None => (
            0usize,
            0usize,
            "No-op: insufficient messages to summarize.".to_string(),
        ),
    };

    Ok(UiEvent::CompactionTriggered {
        source: source.as_str().to_string(),
        summarized_count,
        kept_recent_count,
        summary_preview: summary_preview_text(&summary_body),
        summary_body,
    })
}

/// Execute compaction using rig memory and ConversationStore.
///
/// This async function:
/// 1. Loads messages from InMemoryConversationMemory
/// 2. Calls the summarizer with old rig messages
/// 3. Compacts using `Session::compact`
/// 4. Updates memory and persists to store
///
/// # Arguments
/// * `runtime` - Tokio runtime for async operations
/// * `session` - Session to compact
/// * `memory` - InMemoryConversationMemory containing messages
/// * `store` - ConversationStore for persistence
/// * `summarizer` - Function that takes rig messages and returns summary
/// * `mode` - Compaction invocation mode (Threshold or Force)
///
/// # Returns
/// Ok(Some(outcome)) on successful compaction, Ok(None) if no compaction needed
async fn execute_compaction<S, U>(
    session: &mut Session,
    memory: &rig::memory::InMemoryConversationMemory,
    store: &S,
    client: &rig::providers::copilot::Client,
    model_name: &str,
    mode: CompactionInvocationMode,
    ui: &mut U,
) -> Result<Option<CompactionOutcome>, String>
where
    S: ConversationStore,
    U: ProgressUi,
{
    use rig::memory::ConversationMemory;

    // Load messages from memory to check threshold
    let messages = memory
        .load(session.id())
        .await
        .map_err(|e| format!("Failed to load messages from memory: {}", e))?;

    // Determine if compaction should run
    let should_compact = match mode {
        CompactionInvocationMode::Threshold => {
            messages.len() > session.config().compaction_threshold
        }
        CompactionInvocationMode::Force => true,
    };

    if !should_compact {
        return Ok(None);
    }

    // Perform compaction with summarizer closure
    let summarizer = |old_messages: &[rig::completion::Message]| {
        let messages = old_messages.to_vec();
        async move { summarize_messages(client, model_name, ui, &messages).await }
    };

    let outcome = session
        .compact(memory, store, summarizer)
        .await
        .map_err(|_| COMPACTION_FAILURE_WARNING.to_string())?;

    if outcome.summarized_count == 0 {
        return Ok(None);
    }

    Ok(Some(outcome))
}

fn summary_preview_text(summary_body: &str) -> String {
    let one_line = summary_body.replace('\n', " ");
    one_line.chars().take(120).collect()
}

/// Format rig messages for summarization.
///
/// Extracts text content from rig::completion::Message variants:
/// - Message::User { content } -> text from UserContent::Text
/// - Message::Assistant { content } -> text from AssistantContent::Text  
/// - Message::System { content } -> content string
///
/// Returns formatted string with role: content pairs.
fn format_messages_for_summary(messages: &[rig::completion::Message]) -> String {
    use rig::completion::message::{AssistantContent, UserContent};

    messages
        .iter()
        .map(|msg| {
            let role = match msg {
                rig::completion::Message::User { .. } => "user",
                rig::completion::Message::Assistant { .. } => "assistant",
                rig::completion::Message::System { .. } => "system",
            };

            let content = match msg {
                rig::completion::Message::User { content } => {
                    // Extract text from OneOrMany<UserContent>
                    content
                        .iter()
                        .filter_map(|c| match c {
                            UserContent::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                rig::completion::Message::Assistant { content, .. } => {
                    // Extract text from OneOrMany<AssistantContent>
                    content
                        .iter()
                        .filter_map(|c| match c {
                            AssistantContent::Text(text) => Some(text.text.as_str()),
                            AssistantContent::ToolCall(_) => None,
                            AssistantContent::Reasoning(_) => None,
                            AssistantContent::Image(_) => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                rig::completion::Message::System { content } => content.clone(),
            };

            format!("{}: {}", role, content)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Summarize old rig messages with LLM.
///
/// Formats rig messages, creates summarization prompt, and calls rig agent completion.
async fn summarize_messages<U: ProgressUi>(
    client: &rig::providers::copilot::Client,
    model_name: &str,
    ui: &mut U,
    old_messages: &[rig::completion::Message],
) -> std::io::Result<String> {
    use rig::client::CompletionClient;
    use rig::completion::Completion;

    let history = format_messages_for_summary(old_messages);
    let prompt_text = format!(
        "Summarize the following prior conversation segment concisely while preserving critical decisions, constraints, and open tasks.\n\n{}",
        history
    );

    // Build rig agent from client and model
    let model = client.completion_model(model_name);
    let agent = rig::agent::AgentBuilder::new(model).build();

    // Create the completion future
    let call_result = agent
        .completion(&prompt_text, Vec::<rig::completion::Message>::new())
        .await
        .map_err(|e| std::io::Error::other(format!("{}", e)))?
        .tools(vec![])
        .send();

    let mut call_fut = std::pin::pin!(call_result);

    // Cancellation loop - poll completion future with periodic UI ticks
    loop {
        if ui.take_cancel_requested() {
            return Err(std::io::Error::other("Compaction cancelled by user"));
        }

        enum CompletionProgress<R> {
            Tick,
            Done(
                Box<Result<rig::completion::CompletionResponse<R>, rig::completion::CompletionError>>,
            ),
        }

        match tokio::select! {
            response = &mut call_fut => CompletionProgress::Done(Box::new(response)),
            _ = tokio::time::sleep(Duration::from_millis(80)) => CompletionProgress::Tick,
        } {
            CompletionProgress::Tick => ui.emit(&UiEvent::Tick),
            CompletionProgress::Done(boxed_result) => {
                let result = *boxed_result;
                // Extract text from CompletionResponse
                return result
                    .map(|response| {
                        use rig::completion::message::AssistantContent;
                        let mut text_parts = Vec::new();
                        for content in response.choice {
                            if let AssistantContent::Text(t) = content {
                                text_parts.push(t.to_string());
                            }
                        }
                        text_parts.join("\n")
                    })
                    .map_err(|_| std::io::Error::other(COMPACTION_FAILURE_WARNING));
            }
        }
    }
}

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
