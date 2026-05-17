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
mod tests {
    use std::cell::Cell;

    use nu_protocol::Span;
    use tempfile::tempdir;

    use super::{
        COMPACTION_FAILURE_WARNING, emit_permissions_startup_summary_once,
        execute_compaction_event_shared, execute_compaction_persisted,
        stage_enabled_mcp_runtime_state,
    };
    use crate::{
        agent::protocol::{
            compaction::CompactionTriggerSource, contracts::ProgressUi, event::UiEvent,
        },
        agent::tools::handler::McpToolRegistry,
        llm::LlmUsage,
        session::{
            CompactionOutcome, Message, MessageRole, MessageUsage, SessionConfig, SessionStore,
            StoredToolCall,
        },
        tools::mcp::{
            client::McpToolDefinition,
            config::{McpServerConfig, McpTransportType},
        },
    };
    use rig::completion::message::AssistantContent;

    fn persisted_assistant_message(response: &crate::llm::LlmResponse) -> Message {
        let mut msg = Message::new(MessageRole::Assistant, response.text.clone()).with_usage(
            MessageUsage::new(
                response.usage.input_tokens,
                response.usage.output_tokens,
                response.usage.total_tokens,
            ),
        );

        // Convert tool calls to StoredToolCall format if present
        if !response.tool_calls.is_empty() {
            let stored_calls: Vec<StoredToolCall> = response
                .tool_calls
                .iter()
                .filter_map(|content| {
                    if let AssistantContent::ToolCall(tc) = content {
                        Some(StoredToolCall {
                            id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.to_string(),
                        })
                    } else {
                        None
                    }
                })
                .collect();

            if !stored_calls.is_empty() {
                msg = msg.with_tool_calls(stored_calls);
            }
        }

        msg
    }

    fn mcp_tool(server: &str, name: &str, raw_name: &str) -> McpToolDefinition {
        McpToolDefinition {
            server: server.to_string(),
            name: name.to_string(),
            raw_name: raw_name.to_string(),
            description: Some(format!("{server}:{raw_name}")),
            parameters: Some(serde_json::json!({"type":"object"})),
        }
    }

    fn tool_definition_named(name: &str) -> rig::completion::ToolDefinition {
        rig::completion::ToolDefinition {
            name: name.to_string(),
            description: format!("tool {name}"),
            parameters: serde_json::json!({"type":"object"}),
        }
    }

    fn mcp_server_config(name: &str, enabled: bool) -> McpServerConfig {
        McpServerConfig {
            name: name.to_string(),
            transport: McpTransportType::Http,
            enabled,
            url: Some("http://localhost:7777/mcp".to_string()),
            headers: std::collections::HashMap::new(),
            command: None,
            cwd: None,
            args: Vec::new(),
            env: std::collections::HashMap::new(),
        }
    }

    #[derive(Default)]
    struct TestProgressUi {
        events: Vec<UiEvent>,
    }

    impl ProgressUi for TestProgressUi {
        fn emit(&mut self, event: &UiEvent) {
            self.events.push(event.clone());
        }

        fn flush(&mut self) {}

        fn take_cancel_requested(&self) -> bool {
            false
        }
    }

    #[test]
    fn persisted_assistant_message_includes_structured_usage_fields() {
        let tmp = tempdir().expect("tempdir");
        let store = SessionStore::new_with_cache_dir(tmp.path().to_path_buf());
        let mut session = store
            .get_or_create(Some("assistant-usage-persist".to_string()))
            .expect("create session");

        let usage = LlmUsage {
            input_tokens: 21,
            output_tokens: 34,
            total_tokens: 55,
            cached_input_tokens: 8,
            cache_creation_input_tokens: 13,
        };

        let response = crate::llm::LlmResponse {
            text: "hello".to_string(),
            usage,
            tool_calls: vec![],
            tool_call_metadata: vec![],
        };

        let assistant = persisted_assistant_message(&response);
        session
            .add_message(&store, assistant)
            .expect("persist message");

        let loaded = store
            .load_session("assistant-usage-persist")
            .expect("load session");
        let persisted = loaded
            .messages()
            .iter()
            .find(|m| m.role() == MessageRole::Assistant)
            .expect("assistant message persisted");

        let persisted_usage = persisted.usage().expect("assistant usage persisted");
        assert_eq!(persisted_usage.input_tokens(), Some(21));
        assert_eq!(persisted_usage.output_tokens(), Some(34));
        assert_eq!(persisted_usage.total_tokens(), Some(55));
    }

    #[test]
    fn manual_and_auto_compaction_share_single_execution_path() {
        let mut ui = TestProgressUi::default();
        let counter = Cell::new(0usize);

        let manual = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
            counter.set(counter.get() + 1);
            Ok(Some(CompactionOutcome {
                summarized_count: 1,
                kept_recent_count: 1,
                summary_text: "summary".to_string(),
            }))
        });
        if let Ok(event) = &manual {
            ui.emit(event);
        }
        let auto = execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
            counter.set(counter.get() + 1);
            Ok(Some(CompactionOutcome {
                summarized_count: 1,
                kept_recent_count: 1,
                summary_text: "summary".to_string(),
            }))
        });
        if let Ok(event) = &auto {
            ui.emit(event);
        }

        assert!(manual.is_ok());
        assert!(auto.is_ok());
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn compaction_event_emits_correct_source_metadata() {
        let mut ui = TestProgressUi::default();

        execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
            Ok(Some(CompactionOutcome {
                summarized_count: 3,
                kept_recent_count: 2,
                summary_text: "auto summary body".to_string(),
            }))
        })
        .map(|event| ui.emit(&event))
        .expect("auto event");
        execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
            Ok(Some(CompactionOutcome {
                summarized_count: 4,
                kept_recent_count: 1,
                summary_text: "manual summary body".to_string(),
            }))
        })
        .map(|event| ui.emit(&event))
        .expect("manual event");

        assert!(ui.events.contains(&UiEvent::CompactionTriggered {
            source: "auto_threshold".to_string(),
            summarized_count: 3,
            kept_recent_count: 2,
            summary_preview: "auto summary body".to_string(),
            summary_body: "auto summary body".to_string(),
        }));
        assert!(ui.events.contains(&UiEvent::CompactionTriggered {
            source: "slash_compact".to_string(),
            summarized_count: 4,
            kept_recent_count: 1,
            summary_preview: "manual summary body".to_string(),
            summary_body: "manual summary body".to_string(),
        }));
    }

    #[test]
    fn compaction_summary_transcript_includes_source_and_counts() {
        let mut ui = TestProgressUi::default();

        execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
            Ok(Some(CompactionOutcome {
                summarized_count: 7,
                kept_recent_count: 3,
                summary_text: "summary body for transcript".to_string(),
            }))
        })
        .map(|event| ui.emit(&event))
        .expect("event");

        assert!(ui.events.contains(&UiEvent::CompactionTriggered {
            source: "auto_threshold".to_string(),
            summarized_count: 7,
            kept_recent_count: 3,
            summary_preview: "summary body for transcript".to_string(),
            summary_body: "summary body for transcript".to_string(),
        }));
    }

    #[test]
    fn sliding_summary_compaction_failure_warning_text_is_source_consistent() {
        let tmp = tempdir().expect("tempdir");
        let store = SessionStore::new_with_cache_dir(tmp.path().to_path_buf());
        let mut session = store
            .get_or_create(Some("failure-warning-consistent".to_string()))
            .expect("create session");
        session.set_config(SessionConfig {
            compaction_threshold: 1,
            keep_recent: 1,
            ..SessionConfig::default()
        });
        session
            .add_message(&store, Message::new(MessageRole::User, "a".to_string()))
            .expect("message");
        session
            .add_message(
                &store,
                Message::new(MessageRole::Assistant, "b".to_string()),
            )
            .expect("message");

        let manual = execute_compaction_persisted(
            &mut session,
            &store,
            |_old| Err(std::io::Error::other("manual-source-failure")),
            crate::session::CompactionInvocationMode::Force,
        );
        let auto = execute_compaction_persisted(
            &mut session,
            &store,
            |_old| Err(std::io::Error::other("auto-source-failure")),
            crate::session::CompactionInvocationMode::Threshold,
        );

        assert_eq!(
            manual.expect_err("manual error"),
            COMPACTION_FAILURE_WARNING.to_string()
        );
        assert_eq!(
            auto.expect_err("auto error"),
            COMPACTION_FAILURE_WARNING.to_string()
        );
    }

    #[test]
    fn manual_and_auto_compaction_failure_surface_is_consistent() {
        let manual = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
            Err("Session compaction failed: disk full".to_string())
        });
        let auto = execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
            Err("Session compaction failed: disk full".to_string())
        });

        assert_eq!(manual, auto);
    }

    #[test]
    fn manual_compaction_persists_session_file_updates() {
        let tmp = tempdir().expect("tempdir");
        let store = SessionStore::new_with_cache_dir(tmp.path().to_path_buf());
        let mut session = store
            .get_or_create(Some("manual-compact-persists".to_string()))
            .expect("create session");
        session.set_config(SessionConfig {
            compaction_threshold: 2,
            keep_recent: 1,
            ..SessionConfig::default()
        });

        session
            .add_message(&store, Message::new(MessageRole::User, "a".to_string()))
            .expect("message");
        session
            .add_message(
                &store,
                Message::new(MessageRole::Assistant, "b".to_string()),
            )
            .expect("message");
        session
            .add_message(&store, Message::new(MessageRole::User, "c".to_string()))
            .expect("message");

        let mut ui = TestProgressUi::default();
        execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
            execute_compaction_persisted(
                &mut session,
                &store,
                |_old| Ok("summary".to_string()),
                crate::session::CompactionInvocationMode::Force,
            )
        })
        .map(|event| ui.emit(&event))
        .expect("manual compaction");

        let loaded = store
            .load_session("manual-compact-persists")
            .expect("reload session");
        assert!(loaded.compaction_count() > 0);
    }

    #[test]
    fn auto_compaction_persists_session_file_updates() {
        let tmp = tempdir().expect("tempdir");
        let store = SessionStore::new_with_cache_dir(tmp.path().to_path_buf());
        let mut session = store
            .get_or_create(Some("auto-compact-persists".to_string()))
            .expect("create session");
        session.set_config(SessionConfig {
            compaction_threshold: 2,
            keep_recent: 1,
            ..SessionConfig::default()
        });

        session
            .add_message(&store, Message::new(MessageRole::User, "a".to_string()))
            .expect("message");
        session
            .add_message(
                &store,
                Message::new(MessageRole::Assistant, "b".to_string()),
            )
            .expect("message");
        session
            .add_message(&store, Message::new(MessageRole::User, "c".to_string()))
            .expect("message");

        let mut ui = TestProgressUi::default();
        execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
            execute_compaction_persisted(
                &mut session,
                &store,
                |_old| Ok("summary".to_string()),
                crate::session::CompactionInvocationMode::Threshold,
            )
        })
        .map(|event| ui.emit(&event))
        .expect("auto compaction");

        let loaded = store
            .load_session("auto-compact-persists")
            .expect("reload session");
        assert!(loaded.compaction_count() > 0);
        let _ = Span::test_data();
    }

    #[test]
    fn permissions_startup_summary_emits_once_before_first_turn() {
        let mut ui = TestProgressUi::default();
        let mut emitted = false;
        let summary = "permissions policy: overlay_active=false global=ask tool_rules=5 nu__run.command_rules=1";

        emit_permissions_startup_summary_once(&mut ui, &mut emitted, summary);
        emit_permissions_startup_summary_once(&mut ui, &mut emitted, summary);

        let warnings = ui
            .events
            .iter()
            .filter(|e| matches!(e, UiEvent::Warning { .. }))
            .count();
        assert_eq!(warnings, 1);

        let warning_message = ui
            .events
            .iter()
            .find_map(|event| match event {
                UiEvent::Warning { message } => Some(message.clone()),
                _ => None,
            })
            .expect("warning event");
        assert_eq!(warning_message, summary);
    }

    #[test]
    fn enabling_startup_disabled_server_materializes_filtered_mcp_tools_for_current_session() {
        let mut tool_definitions = vec![tool_definition_named("read")];
        let mut registry =
            McpToolRegistry::from_tools(vec![mcp_tool("gh", "gh__list_prs", "list_prs")])
                .expect("startup registry");

        let discovered_from_toggle = vec![
            mcp_tool("k8s", "k8s__list_pods", "list_pods"),
            mcp_tool("k8s", "k8s__delete_pod", "delete_pod"),
        ];

        super::merge_new_mcp_tools_into_runtime(
            &mut tool_definitions,
            &mut registry,
            &discovered_from_toggle,
            &["k8s__list_*".to_string()],
        )
        .expect("toggle merge should succeed");

        let visible = crate::agent::tools::handler::llm_visible_tool_definitions(
            &tool_definitions,
            &registry,
        );

        assert!(visible.iter().any(|tool| tool.name == "k8s__list_pods"));
        assert!(
            visible.iter().all(|tool| tool.name != "k8s__delete_pod"),
            "cli MCP patterns must be applied consistently when enabling servers in-session"
        );
        assert_eq!(
            visible
                .iter()
                .filter(|tool| tool.name.starts_with("k8s__"))
                .count(),
            1
        );
    }

    #[test]
    fn enabling_startup_disabled_server_registers_dispatch_raw_name_mapping() {
        let mut tool_definitions = vec![tool_definition_named("read")];
        let mut registry = McpToolRegistry::from_names(Vec::<String>::new());

        let discovered = vec![mcp_tool("k8s", "k8s__list_pods", "list_pods")];

        super::merge_new_mcp_tools_into_runtime(
            &mut tool_definitions,
            &mut registry,
            &discovered,
            &[],
        )
        .expect("toggle merge should succeed");

        assert_eq!(registry.raw_name_for("k8s__list_pods"), Some("list_pods"));
        assert!(registry.contains("k8s__list_pods"));
    }

    #[test]
    fn enabling_stage_conflict_is_transactional_and_keeps_original_runtime_state() {
        let tool_definitions = vec![tool_definition_named("read")];
        let registry =
            McpToolRegistry::from_tools(vec![mcp_tool("gh", "k8s__list_pods", "list_pods")])
                .expect("startup registry");

        let discovered_conflict = vec![mcp_tool("k8s", "k8s__list_pods", "list_all_pods")];

        let result = stage_enabled_mcp_runtime_state(
            &tool_definitions,
            &registry,
            "k8s",
            &discovered_conflict,
            &[],
        );

        assert!(result.is_err());
        assert!(
            result
                .expect_err("must fail on conflicting raw mapping")
                .contains("conflicting raw MCP tool mapping")
        );

        assert_eq!(
            tool_definitions.len(),
            1,
            "tool definitions must remain unchanged"
        );
        assert_eq!(tool_definitions[0].name, "read");

        assert!(
            registry.contains("k8s__list_pods"),
            "existing registry mapping must remain visible"
        );
        assert_eq!(registry.raw_name_for("k8s__list_pods"), Some("list_pods"));
        assert!(
            !registry.is_server_enabled("k8s"),
            "new server must not be enabled on conflict"
        );
    }

    #[test]
    fn lifecycle_projection_recomputes_from_registry_state_without_runtime() {
        let registry = McpToolRegistry::from_tools(vec![
            mcp_tool("gh", "gh__list_prs", "list_prs"),
            mcp_tool("k8s", "k8s__list_pods", "list_pods"),
        ])
        .expect("registry");
        registry
            .set_server_enabled("k8s", false)
            .expect("disable k8s");

        let configs = vec![
            mcp_server_config("k8s", true),
            mcp_server_config("gh", true),
        ];

        let projection = super::rebuild_mcp_lifecycle_projection(
            None,
            &configs,
            &registry,
            &[
                tool_definition_named("read"),
                tool_definition_named("gh__list_prs"),
                tool_definition_named("k8s__list_pods"),
            ],
        );

        assert_eq!(projection.len(), 2);
        assert_eq!(projection[0].name, "gh");
        assert!(projection[0].enabled);
        assert!(!projection[0].connected);
        assert_eq!(projection[0].visible_tool_count, 1);

        assert_eq!(projection[1].name, "k8s");
        assert!(!projection[1].enabled);
        assert!(!projection[1].connected);
        assert_eq!(projection[1].visible_tool_count, 0);
    }

    // ========================================================================
    // Structured messages tests
    // ========================================================================

    #[test]
    fn build_system_preamble_joins_non_empty_parts() {
        let result = super::build_system_preamble(
            Some("preamble text"),
            Some("context text"),
            Some("agents chain"),
            Some("available skills"),
        );

        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("preamble text"));
        assert!(text.contains("context text"));
        assert!(text.contains("agents chain"));
        assert!(text.contains("available skills"));
    }

    #[test]
    fn build_system_preamble_returns_none_when_all_empty() {
        let result = super::build_system_preamble(None, None, None, None);
        assert!(result.is_none());
    }

    #[test]
    fn build_system_preamble_handles_partial_inputs() {
        let result = super::build_system_preamble(Some("preamble"), None, Some("agents"), None);

        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("preamble"));
        assert!(text.contains("agents"));
    }
}
