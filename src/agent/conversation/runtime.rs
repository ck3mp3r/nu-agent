use std::time::Duration;

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};

use crate::{
    config::Config,
    plugin::RuntimeCtx,
    session::{CompactionInvocationMode, CompactionOutcome, Message, Session, SessionStore},
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
    pub session: Option<Session>,
    pub final_session_id: Option<String>,
    pub auto_compaction_tolerance: usize,
    pub auto_compaction_hysteresis_margin: usize,
    pub auto_compaction_state: CompactionTriggerState,
    pub startup_plugin_config: Option<crate::config::PluginConfig>,
    pub permissions: PermissionsConfig,
    pub permissions_startup_summary: String,
    pub permissions_startup_emitted: bool,
    pub session_grants: SessionGrantCache,
    pub ask_hook: AsyncAskHook,
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
        let Some(session) = self.session.as_ref() else {
            return Some(CompactionTriggerDecision::NoFire {
                reason: "signal_unavailable".to_string(),
            });
        };

        let policy = ThresholdCompactionPolicy::new(
            session.config().compaction_threshold,
            self.auto_compaction_tolerance,
            self.auto_compaction_hysteresis_margin,
        );
        Some(policy.evaluate(
            Some(session.messages().len()),
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
        use crate::providers::github_copilot::model::agent_from_config;

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

        // Get session history as structured messages
        let session_history = if let Some(ref session) = self.session {
            session.as_chat_history()
        } else {
            Vec::new()
        };

        // Create the GitHub Copilot agent from config
        let agent = agent_from_config(
            &self.config.provider,
            &self.config.model,
            self.config.api_key.clone(),
            self.config.base_url.clone(),
        )
        .map_err(|e| {
            LabeledError::new(format!("Failed to create agent: {}", e))
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
                agent: &agent,
                prompt,
                session_history,
                preamble: preamble.as_deref(),
                max_turns: self.config.max_tool_turns,
                session: self.session.as_mut(),
                store: Some(&self.store),
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

        // Format the response value
        let mut message_count = 0;
        let mut compaction_count = 0;

        if self.session.is_some() {
            if let Some(CompactionTriggerDecision::Fire { source, .. }) =
                self.evaluate_auto_compaction()
                && let Err(error) = self.execute_compaction_event(ui, source)
            {
                ui.emit(&UiEvent::Warning { message: error });
            }

            if let Some(session) = self.session.as_ref() {
                message_count = session.messages().len();
                compaction_count = session.compaction_count();
            }
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
        use crate::providers::github_copilot::model::agent_from_config;

        let runtime = &self.runtime;
        let store = &self.store;

        // Create the GitHub Copilot agent for compaction
        let agent = agent_from_config(
            &self.config.provider,
            &self.config.model,
            self.config.api_key.clone(),
            self.config.base_url.clone(),
        )
        .map_err(|e| format!("Failed to create agent for compaction: {}", e))?;

        let source_label = source.as_str().to_string();
        ui.emit(&UiEvent::CompactionStarted {
            source: source_label.clone(),
        });
        let result = execute_compaction_event_shared(source, || {
            let session = self
                .session
                .as_mut()
                .ok_or_else(|| "session_unavailable".to_string())?;
            let mode = match source {
                CompactionTriggerSource::SlashCompact => CompactionInvocationMode::Force,
                CompactionTriggerSource::AutoThreshold => CompactionInvocationMode::Threshold,
            };
            execute_compaction_persisted(
                session,
                store,
                |old_messages| summarize_old_segment_with_llm(runtime, &agent, ui, old_messages),
                mode,
            )
        });
        match result {
            Ok(event) => {
                ui.emit(&event);
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

fn execute_compaction_persisted<F>(
    session: &mut Session,
    store: &SessionStore,
    summarizer: F,
    mode: CompactionInvocationMode,
) -> Result<Option<CompactionOutcome>, String>
where
    F: FnOnce(&[Message]) -> std::io::Result<String>,
{
    session
        .maybe_compact_with_mode(store, mode, summarizer)
        .map_err(|_| COMPACTION_FAILURE_WARNING.to_string())
}

fn summary_preview_text(summary_body: &str) -> String {
    let one_line = summary_body.replace('\n', " ");
    one_line.chars().take(120).collect()
}

fn summarize_old_segment_with_llm<U: ProgressUi>(
    runtime: &tokio::runtime::Runtime,
    agent: &crate::providers::github_copilot::model::Agent,
    ui: &mut U,
    old_messages: &[Message],
) -> std::io::Result<String> {
    let history = old_messages
        .iter()
        .map(|message| format!("{}: {}", message.role(), message.content()))
        .collect::<Vec<_>>()
        .join("\n\n");
    let prompt_text = format!(
        "Summarize the following prior conversation segment concisely while preserving critical decisions, constraints, and open tasks.\n\n{}",
        history
    );

    // Create the completion future using agent.completion()
    let mut call_fut = std::pin::pin!(agent.completion(&prompt_text));

    // Cancellation loop - poll completion future with periodic UI ticks
    loop {
        if ui.take_cancel_requested() {
            return Err(std::io::Error::other("Compaction cancelled by user"));
        }

        enum CompletionProgress {
            Tick,
            Done(Result<String, rig::completion::CompletionError>),
        }

        match runtime.block_on(async {
            tokio::select! {
                response = &mut call_fut => CompletionProgress::Done(response),
                _ = tokio::time::sleep(Duration::from_millis(80)) => CompletionProgress::Tick,
            }
        }) {
            CompletionProgress::Tick => ui.emit(&UiEvent::Tick),
            CompletionProgress::Done(result) => {
                return result.map_err(|_| std::io::Error::other(COMPACTION_FAILURE_WARNING));
            }
        }
    }
}

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
