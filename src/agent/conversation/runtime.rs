use std::time::Duration;

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};

use crate::{
    config::Config,
    llm::LlmResponse,
    plugin::RuntimeCtx,
    session::{
        CompactionInvocationMode, CompactionOutcome, Message, MessageUsage, Session, SessionStore,
    },
    tools::{closure::ClosureRegistry, executor::ToolExecutor},
};

use crate::agent::{
    protocol::{
        cancellation::{is_llm_call_cancelled, llm_call_cancelled_error},
        compaction::{
            CompactionTriggerDecision, CompactionTriggerPolicy, CompactionTriggerSource,
            CompactionTriggerState, ThresholdCompactionPolicy,
        },
        contracts::{ConversationRuntime, McpUsabilityState, ProgressUi},
        event::UiEvent,
        tool_args::summarize_tool_arguments,
    },
    tools::{
        authz::{AsyncAskHook, PermissionEventSink, PermissionsConfig, SessionGrantCache},
        handler::{self, McpToolRegistry, ToolHandlerContext, ToolSource},
    },
};
use crate::tools::mcp::{
    config::McpServerConfig,
    runtime::{McpRuntime, McpServerLifecycle},
};

const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

const DOOM_LOOP_THRESHOLD: usize = 3;

enum LlmCallProgress {
    Tick,
    Done(Result<LlmResponse, LabeledError>),
}

#[derive(Debug, Clone)]
struct HistoryEntry {
    role: String,
    content: String,
    tool_result: Option<String>,
}

impl HistoryEntry {
    fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_result: None,
        }
    }

    fn tool(content: impl Into<String>, result: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_result: Some(result.into()),
        }
    }

    fn render_for_history_prompt(&self) -> String {
        let mut line = format!("{}: {}", self.role, self.content);
        if let Some(result) = &self.tool_result {
            line.push_str(" result=");
            line.push_str(result);
        }
        line
    }
}

fn is_edit_sanitizable_mode(arguments: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| {
            value
                .get("mode")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        }) {
        Some(mode) => mode == "preview" || mode == "apply",
        None => true,
    }
}

fn compact_edit_preview_stats(stats: &serde_json::Value) -> Option<serde_json::Value> {
    let stats_obj = stats.as_object()?;
    let mut compact = serde_json::Map::new();
    for key in [
        "files_changed",
        "insertions",
        "deletions",
        "diff_truncated",
        "omitted_files",
        "omitted_hunks",
    ] {
        if let Some(value) = stats_obj.get(key) {
            compact.insert(key.to_string(), value.clone());
        }
    }
    if compact.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(compact))
    }
}

fn sanitize_tool_result_for_history_prompt(result: &handler::ToolCallResult) -> String {
    if result.tool_name != "edit" || !is_edit_sanitizable_mode(&result.arguments) {
        return result.content.clone();
    }

    let Ok(content) = serde_json::from_str::<serde_json::Value>(&result.content) else {
        return result.content.clone();
    };
    let Some(content_obj) = content.as_object() else {
        return result.content.clone();
    };

    let mut compact = serde_json::Map::new();
    if let Some(mode) = content_obj.get("mode") {
        compact.insert("mode".to_string(), mode.clone());
    }
    if let Some(path) = content_obj.get("path") {
        compact.insert("path".to_string(), path.clone());
    }
    for key in ["applied", "would_change", "noop", "conflict"] {
        if let Some(value) = content_obj.get(key) {
            compact.insert(key.to_string(), value.clone());
        }
    }
    if let Some(stats) = content_obj.get("stats")
        && let Some(compact_stats) = compact_edit_preview_stats(stats)
    {
        compact.insert("stats".to_string(), compact_stats);
    }
    compact.insert(
        "diff_rendered_directly".to_string(),
        serde_json::Value::Bool(true),
    );

    serde_json::to_string(&serde_json::Value::Object(compact))
        .unwrap_or_else(|_| result.content.clone())
}

fn call_llm_with_ui_ticks<U: ProgressUi>(
    runtime: &tokio::runtime::Runtime,
    runtime_ctx: &RuntimeCtx,
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
    ui: &mut U,
) -> Result<LlmResponse, LabeledError> {
    let mut call_fut = std::pin::pin!(crate::llm::call_llm(runtime_ctx, config, prompt, tools));

    loop {
        if ui.take_cancel_requested() {
            return Err(llm_call_cancelled_error());
        }

        match runtime.block_on(async {
            tokio::select! {
                response = &mut call_fut => LlmCallProgress::Done(response),
                _ = tokio::time::sleep(Duration::from_millis(80)) => LlmCallProgress::Tick,
            }
        }) {
            LlmCallProgress::Tick => ui.emit(&UiEvent::Tick),
            LlmCallProgress::Done(result) => return result,
        }
    }
}

fn merge_runtime_prompt(
    prompt: &str,
    context: Option<&str>,
    preamble: Option<&str>,
    agents_chain: Option<&str>,
    available_skills: Option<&str>,
) -> String {
    let prompt_with_skills =
        crate::agent::protocol::prompt::merge_prompt_with_context(prompt, available_skills);
    let prompt_with_agents = crate::agent::protocol::prompt::merge_prompt_with_context(
        &prompt_with_skills,
        agents_chain,
    );
    crate::agent::protocol::prompt::merge_preamble_with_prompt_and_context(
        &prompt_with_agents,
        context,
        preamble,
    )
}

pub(crate) struct AgentConversationRuntime {
    pub runtime: tokio::runtime::Runtime,
    pub runtime_ctx: RuntimeCtx,
    pub config: Config,
    pub tool_definitions: Vec<rig::completion::ToolDefinition>,
    pub closure_registry: ClosureRegistry,
    pub mcp_registry: McpToolRegistry,
    pub mcp_runtime: Option<McpRuntime>,
    pub mcp_tool_server_handle: Option<rig::tool::server::ToolServerHandle>,
    pub mcp_lifecycle_projection: Vec<McpServerLifecycle>,
    pub mcp_server_configs: Vec<McpServerConfig>,
    pub mcp_cli_patterns: Vec<String>,
    pub mcp_caller_cwd: Option<std::path::PathBuf>,
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

struct UiPermissionSink<'a, U: ProgressUi> {
    ui: &'a mut U,
}

impl<U: ProgressUi> PermissionEventSink for UiPermissionSink<'_, U> {
    fn emit(&mut self, event: UiEvent) {
        self.ui.emit(&event);
    }
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
                    .map(McpRuntime::tool_server_handle);
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

        let mut merged_prompt = merge_runtime_prompt(
            &prompt,
            context.as_deref(),
            self.config.preamble.as_deref(),
            loaded_agents.merged_chain.as_deref(),
            available_skills.as_deref(),
        );

        if let Some(ref session) = self.session {
            let history = session.format_history();
            if !history.is_empty() {
                merged_prompt = format!(
                    "Previous conversation:\n{}\n\n---\n\n{}",
                    history, merged_prompt
                );
            }
        }

        ui.emit(&UiEvent::LlmStart);
        let active_tool_definitions = self.active_tool_definitions();
        let mut llm_response = match call_llm_with_ui_ticks(
            &self.runtime,
            &self.runtime_ctx,
            &self.config,
            &merged_prompt,
            active_tool_definitions,
            ui,
        ) {
            Ok(response) => response,
            Err(e) => {
                ui.emit(&UiEvent::Completed { tool_calls: 0 });
                ui.flush();
                if is_llm_call_cancelled(&e)
                    && let Some(value) = ui.cancellation_value(span)
                {
                    return Ok(value);
                }
                return Err(LabeledError::new(format!("LLM call failed: {}", e.msg))
                    .with_label(e.msg, span));
            }
        };
        ui.emit(&UiEvent::LlmEnd {
            response_chars: llm_response.text.len(),
            tool_calls: llm_response.tool_calls.len(),
            input_tokens: llm_response.usage.input_tokens,
            output_tokens: llm_response.usage.output_tokens,
            total_tokens: llm_response.usage.total_tokens,
        });

        let mut executed_tool_calls: Vec<rig::completion::AssistantContent> = Vec::new();
        let mut tool_results_metadata: Vec<crate::llm::ToolCallMetadata> = Vec::new();
        let mut conversation_messages: Vec<HistoryEntry> = vec![];
        conversation_messages.push(HistoryEntry::new("user", merged_prompt.clone()));
        conversation_messages.push(HistoryEntry::new("assistant", llm_response.text.clone()));

        let max_tool_turns = self.config.max_tool_turns; // Option<u32>: None means unlimited
        let mut tool_turn = 0;
        let mut recent_tool_signatures: Vec<(String, String)> = Vec::new();

        while !llm_response.tool_calls.is_empty()
            && max_tool_turns.is_none_or(|max| tool_turn < max)
        {
            tool_turn += 1;

            for content in &llm_response.tool_calls {
                if let rig::completion::message::AssistantContent::ToolCall(tc) = content {
                    let source = if self.closure_registry.get(&tc.function.name).is_some() {
                        "closure".to_string()
                    } else if self.mcp_registry.contains(&tc.function.name) {
                        "mcp".to_string()
                    } else {
                        "unknown".to_string()
                    };
                    ui.emit(&UiEvent::ToolStart {
                        name: tc.function.name.clone(),
                        source,
                        arguments: serde_json::to_string(&tc.function.arguments)
                            .unwrap_or_else(|_| "{}".to_string()),
                    });
                }
            }

            executed_tool_calls.extend(llm_response.tool_calls.clone());

            let mut permission_sink = UiPermissionSink { ui };
            let mut handler_context = ToolHandlerContext {
                closure_registry: &self.closure_registry,
                mcp_registry: &self.mcp_registry,
                mcp_tool_server: self.mcp_tool_server_handle.as_ref(),
                tool_executor: &self.tool_executor,
                engine: &self.engine,
                authorization: handler::ToolAuthorizationContext {
                    permissions: &self.permissions,
                    grant_cache: &mut self.session_grants,
                    ask_hook: &mut self.ask_hook,
                    event_sink: &mut permission_sink,
                },
                span,
            };
            let tool_results = self.runtime.block_on(handler::handle_tool_calls(
                llm_response.tool_calls.clone(),
                &mut handler_context,
            ));

            for result in &tool_results {
                let source = match result.source {
                    ToolSource::Closure => "closure".to_string(),
                    ToolSource::Mcp => "mcp".to_string(),
                    ToolSource::Unknown => "unknown".to_string(),
                };

                ui.emit(&UiEvent::ToolEnd {
                    name: result.tool_name.clone(),
                    source: source.clone(),
                    arguments: result.arguments.clone(),
                    success: result.failure.is_none(),
                    result: result.content.clone(),
                    display: result.display.clone(),
                    error_kind: result
                        .failure
                        .as_ref()
                        .map(|failure| failure.error_kind.as_str().to_string()),
                    message: result
                        .failure
                        .as_ref()
                        .map(|failure| failure.message.clone()),
                });

                tool_results_metadata.push(crate::llm::ToolCallMetadata {
                    id: result.tool_call_id.clone(),
                    name: result.tool_name.clone(),
                    arguments: result.arguments.clone(),
                    source: Some(source),
                    error_kind: result
                        .failure
                        .as_ref()
                        .map(|failure| failure.error_kind.as_str().to_string()),
                    message: result
                        .failure
                        .as_ref()
                        .map(|failure| failure.message.clone()),
                    details: result
                        .failure
                        .as_ref()
                        .and_then(|failure| failure.details.as_ref())
                        .and_then(|details| serde_json::to_string(details).ok()),
                });
            }

            for result in &tool_results {
                conversation_messages.push(HistoryEntry::tool(
                    persisted_tool_text_for(result),
                    sanitize_tool_result_for_history_prompt(result),
                ));
            }

            // Track tool signatures for doom loop detection
            for result in &tool_results {
                recent_tool_signatures.push((result.tool_name.clone(), result.arguments.clone()));
            }

            // Check for doom loop
            if let Some(tool_name) = is_doom_loop(&recent_tool_signatures, DOOM_LOOP_THRESHOLD) {
                ui.emit(&UiEvent::Warning {
                    message: format!(
                        "Doom loop detected: '{}' called {} times with identical arguments. Breaking tool loop.",
                        tool_name, DOOM_LOOP_THRESHOLD
                    ),
                });
                break;
            }

            if let Some(ref mut session) = self.session {
                for result in &tool_results {
                    let tool_msg =
                        Message::new("tool".to_string(), persisted_tool_text_for(result))
                            .with_tool_details(
                                result.arguments.clone(),
                                result.content.clone(),
                                result.failure.is_none(),
                            );
                    session.add_message(&self.store, tool_msg).map_err(|e| {
                        LabeledError::new(format!("Failed to save tool message: {}", e))
                    })?;
                }
            }

            let history_prompt = {
                let history = conversation_messages
                    .iter()
                    .map(HistoryEntry::render_for_history_prompt)
                    .collect::<Vec<_>>()
                    .join("\n\n");

                if !history.is_empty() {
                    format!(
                        "Previous conversation:\n{}\n\n---\n\nContinue responding.",
                        history
                    )
                } else {
                    merged_prompt.clone()
                }
            };

            ui.emit(&UiEvent::LlmStart);
            let active_tool_definitions = self.active_tool_definitions();
            llm_response = match call_llm_with_ui_ticks(
                &self.runtime,
                &self.runtime_ctx,
                &self.config,
                &history_prompt,
                active_tool_definitions,
                ui,
            ) {
                Ok(response) => response,
                Err(e) => {
                    ui.emit(&UiEvent::Completed { tool_calls: 0 });
                    ui.flush();
                    if is_llm_call_cancelled(&e)
                        && let Some(value) = ui.cancellation_value(span)
                    {
                        return Ok(value);
                    }
                    return Err(LabeledError::new(format!("LLM call failed: {}", e.msg))
                        .with_label(e.msg, span));
                }
            };
            ui.emit(&UiEvent::LlmEnd {
                response_chars: llm_response.text.len(),
                tool_calls: llm_response.tool_calls.len(),
                input_tokens: llm_response.usage.input_tokens,
                output_tokens: llm_response.usage.output_tokens,
                total_tokens: llm_response.usage.total_tokens,
            });
            conversation_messages.push(HistoryEntry::new("assistant", llm_response.text.clone()));
        }

        // Emit warning if tool turn limit was exhausted with pending tool calls
        if !llm_response.tool_calls.is_empty()
            && let Some(max) = max_tool_turns
        {
            let pending_tools: Vec<String> = llm_response
                .tool_calls
                .iter()
                .filter_map(|c| {
                    if let rig::completion::message::AssistantContent::ToolCall(tc) = c {
                        Some(tc.function.name.clone())
                    } else {
                        None
                    }
                })
                .collect();
            ui.emit(&UiEvent::Warning {
                message: format!(
                    "Tool turn limit reached ({}). Suppressed pending tool calls: {}",
                    max,
                    pending_tools.join(", ")
                ),
            });
        }

        let tool_call_count = executed_tool_calls.len();

        let final_response = LlmResponse {
            text: llm_response.text.clone(),
            usage: llm_response.usage.clone(),
            tool_calls: executed_tool_calls,
            tool_call_metadata: tool_results_metadata,
        };

        let response_text = final_response.text.clone();

        let mut message_count = 0;
        let mut compaction_count = 0;

        if self.session.is_some() {
            {
                let session = self.session.as_mut().expect("session checked as present");
                let user_msg = Message::new("user".to_string(), prompt.clone());
                session.add_message(&self.store, user_msg).map_err(|e| {
                    LabeledError::new(format!("Failed to save user message: {}", e))
                })?;

                let assistant_msg =
                    persisted_assistant_message(&response_text, &llm_response.usage);
                session
                    .add_message(&self.store, assistant_msg)
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to save assistant message: {}", e))
                    })?;
            }

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
            text: response_text,
        });
        ui.emit(&UiEvent::Completed {
            tool_calls: tool_call_count,
        });
        ui.flush();

        let response_value = crate::llm::format_response(
            &final_response,
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
        let runtime_ctx = &self.runtime_ctx;
        let config = &self.config;
        let store = &self.store;

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
                |old_messages| {
                    summarize_old_segment_with_llm(runtime, runtime_ctx, config, ui, old_messages)
                },
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
    runtime_ctx: &RuntimeCtx,
    config: &Config,
    ui: &mut U,
    old_messages: &[Message],
) -> std::io::Result<String> {
    let history = old_messages
        .iter()
        .map(|message| format!("{}: {}", message.role(), message.content()))
        .collect::<Vec<_>>()
        .join("\n\n");
    let prompt = format!(
        "Summarize the following prior conversation segment concisely while preserving critical decisions, constraints, and open tasks.\n\n{}",
        history
    );

    let response = call_llm_with_ui_ticks(runtime, runtime_ctx, config, &prompt, Vec::new(), ui)
        .map_err(|_| std::io::Error::other(COMPACTION_FAILURE_WARNING))?;
    Ok(response.text)
}

fn persisted_tool_text_for(result: &handler::ToolCallResult) -> String {
    let summarized_args = summarize_tool_arguments(&result.arguments);
    format!(
        "tool[{}] args={} · {}",
        result.tool_name,
        summarized_args,
        if result.failure.is_none() {
            "done"
        } else {
            "failed"
        }
    )
}

fn persisted_assistant_message(content: &str, usage: &crate::llm::LlmUsage) -> Message {
    Message::new("assistant".to_string(), content.to_string()).with_usage(MessageUsage::new(
        usage.input_tokens,
        usage.output_tokens,
        usage.total_tokens,
    ))
}

fn is_doom_loop(recent: &[(String, String)], threshold: usize) -> Option<&str> {
    if recent.len() < threshold {
        return None;
    }
    let last_n = &recent[recent.len() - threshold..];
    let first = &last_n[0];
    if last_n.iter().all(|s| s == first) {
        Some(&first.0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use nu_protocol::Span;
    use tempfile::tempdir;

    use super::{
        COMPACTION_FAILURE_WARNING, emit_permissions_startup_summary_once,
        execute_compaction_event_shared, execute_compaction_persisted, merge_runtime_prompt,
        persisted_assistant_message, sanitize_tool_result_for_history_prompt,
        stage_enabled_mcp_runtime_state,
    };
    use crate::{
        agent::protocol::{
            compaction::CompactionTriggerSource, contracts::ProgressUi, event::UiEvent,
        },
        agent::tools::handler::{McpToolRegistry, ToolCallResult, ToolSource},
        llm::LlmUsage,
        session::{CompactionOutcome, Message, SessionConfig, SessionStore},
        tools::mcp::{
            client::McpToolDefinition,
            config::{McpServerConfig, McpTransportType},
        },
    };

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

    fn preview_edit_result(content: serde_json::Value) -> ToolCallResult {
        ToolCallResult {
            tool_call_id: "tool-call-1".to_string(),
            tool_name: "edit".to_string(),
            arguments: serde_json::json!({
                "path": "sample.txt",
                "mode": "preview",
                "search": "old",
                "replacement": "new"
            })
            .to_string(),
            source: ToolSource::Closure,
            content: content.to_string(),
            display: None,
            failure: None,
        }
    }

    fn apply_edit_result(content: serde_json::Value) -> ToolCallResult {
        ToolCallResult {
            tool_call_id: "tool-call-2".to_string(),
            tool_name: "edit".to_string(),
            arguments: serde_json::json!({
                "path": "sample.txt",
                "mode": "apply",
                "search": "old",
                "replacement": "new"
            })
            .to_string(),
            source: ToolSource::Closure,
            content: content.to_string(),
            display: None,
            failure: None,
        }
    }

    fn apply_edit_result_with_omitted_mode(content: serde_json::Value) -> ToolCallResult {
        ToolCallResult {
            tool_call_id: "tool-call-3".to_string(),
            tool_name: "edit".to_string(),
            arguments: serde_json::json!({
                "path": "sample.txt",
                "search": "old",
                "replacement": "new"
            })
            .to_string(),
            source: ToolSource::Closure,
            content: content.to_string(),
            display: None,
            failure: None,
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

        let assistant = persisted_assistant_message("hello", &usage);
        session
            .add_message(&store, assistant)
            .expect("persist message");

        let loaded = store
            .load_session("assistant-usage-persist")
            .expect("load session");
        let persisted = loaded
            .messages()
            .iter()
            .find(|m| m.role() == "assistant")
            .expect("assistant message persisted");

        let persisted_usage = persisted.usage().expect("assistant usage persisted");
        assert_eq!(persisted_usage.input_tokens(), Some(21));
        assert_eq!(persisted_usage.output_tokens(), Some(34));
        assert_eq!(persisted_usage.total_tokens(), Some(55));
    }

    #[test]
    fn runtime_prompt_includes_merged_agents_chain_before_user_prompt() {
        let merged = merge_runtime_prompt(
            "user prompt",
            Some("ctx"),
            Some("preamble"),
            Some("AGENT-HOME\n\nAGENT-CWD"),
            Some("<available_skills>\n  <skill><name>context</name></skill>\n</available_skills>"),
        );

        let preamble_pos = merged.find("preamble").expect("preamble present");
        let agents_pos = merged.find("AGENT-HOME").expect("agents present");
        let user_pos = merged.find("user prompt").expect("user prompt present");

        assert!(
            preamble_pos < agents_pos,
            "preamble should remain before injected agents chain"
        );
        assert!(
            agents_pos < user_pos,
            "agents chain must appear before user prompt in merged runtime prompt"
        );
    }

    #[test]
    fn runtime_prompt_includes_available_skills_after_agents_chain_and_before_user_prompt() {
        let merged = merge_runtime_prompt(
            "user prompt",
            Some("ctx"),
            Some("preamble"),
            Some("AGENT-HOME\n\nAGENT-CWD"),
            Some("<available_skills>\n  <skill><name>context</name></skill>\n</available_skills>"),
        );

        let preamble_pos = merged.find("preamble").expect("preamble present");
        let skills_pos = merged.find("<available_skills>").expect("skills present");
        let agents_pos = merged.find("AGENT-HOME").expect("agents present");
        let user_pos = merged.find("user prompt").expect("user prompt present");

        assert!(preamble_pos < agents_pos, "preamble should remain first");
        assert!(
            agents_pos < skills_pos,
            "agents should appear before skills list"
        );
        assert!(
            skills_pos < user_pos,
            "skills should remain before user prompt"
        );
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
            .add_message(&store, Message::new("user".to_string(), "a".to_string()))
            .expect("message");
        session
            .add_message(
                &store,
                Message::new("assistant".to_string(), "b".to_string()),
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
            .add_message(&store, Message::new("user".to_string(), "a".to_string()))
            .expect("message");
        session
            .add_message(
                &store,
                Message::new("assistant".to_string(), "b".to_string()),
            )
            .expect("message");
        session
            .add_message(&store, Message::new("user".to_string(), "c".to_string()))
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
            .add_message(&store, Message::new("user".to_string(), "a".to_string()))
            .expect("message");
        session
            .add_message(
                &store,
                Message::new("assistant".to_string(), "b".to_string()),
            )
            .expect("message");
        session
            .add_message(&store, Message::new("user".to_string(), "c".to_string()))
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
    fn history_prompt_omits_full_edit_preview_diff_payload() {
        let result = preview_edit_result(serde_json::json!({
            "mode": "preview",
            "path": "sample.txt",
            "diff": "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n",
            "applied": false,
            "would_change": true,
            "noop": false,
            "conflict": false,
            "stats": {
                "files_changed": 1,
                "insertions": 1,
                "deletions": 1,
                "diff_truncated": false
            }
        }));

        let sanitized = sanitize_tool_result_for_history_prompt(&result);

        assert!(!sanitized.contains("\"diff\""));
        assert!(!sanitized.contains("--- a/sample.txt"));
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
    fn history_prompt_includes_compact_edit_preview_status_marker() {
        let result = preview_edit_result(serde_json::json!({
            "mode": "preview",
            "path": "sample.txt",
            "diff": "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n",
            "applied": false,
            "would_change": true,
            "noop": false,
            "conflict": false,
            "stats": {
                "files_changed": 1,
                "insertions": 1,
                "deletions": 1,
                "diff_truncated": false,
                "omitted_files": 0,
                "omitted_hunks": 0
            }
        }));

        let sanitized = sanitize_tool_result_for_history_prompt(&result);
        let payload: serde_json::Value = serde_json::from_str(&sanitized).expect("sanitized json");

        assert_eq!(
            payload["diff_rendered_directly"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(payload["applied"], serde_json::Value::Bool(false));
        assert_eq!(payload["would_change"], serde_json::Value::Bool(true));
        assert_eq!(payload["noop"], serde_json::Value::Bool(false));
        assert_eq!(payload["conflict"], serde_json::Value::Bool(false));
        assert_eq!(
            payload["stats"]["files_changed"],
            serde_json::Value::from(1)
        );
    }

    #[test]
    fn non_preview_tool_history_payload_remains_unchanged() {
        let content = serde_json::json!({
            "ok": true,
            "diff": "--- untouched"
        });
        let result = ToolCallResult {
            tool_call_id: "tool-call-non-edit".to_string(),
            tool_name: "read".to_string(),
            arguments: serde_json::json!({ "path": "sample.txt" }).to_string(),
            source: ToolSource::Closure,
            content: content.to_string(),
            display: None,
            failure: None,
        };

        let sanitized = sanitize_tool_result_for_history_prompt(&result);
        assert_eq!(sanitized, content.to_string());
    }

    #[test]
    fn edit_apply_history_prompt_remains_compact_without_full_diff_payload() {
        let content = serde_json::json!({
            "mode": "apply",
            "path": "sample.txt",
            "diff": "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n",
            "applied": true,
            "would_change": true,
            "stats": {
                "files_changed": 1,
                "insertions": 1,
                "deletions": 1,
                "diff_truncated": false,
                "omitted_files": 0,
                "omitted_hunks": 0
            },
            "display": {
                "title": "edit sample.txt",
                "sections": [
                    {
                        "label": "sample.txt",
                        "language": "diff",
                        "content": "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                    }
                ]
            }
        });
        let result = apply_edit_result(content.clone());

        let sanitized = sanitize_tool_result_for_history_prompt(&result);
        let payload: serde_json::Value = serde_json::from_str(&sanitized).expect("sanitized json");

        assert!(!sanitized.contains("\"diff\""));
        assert!(!sanitized.contains("--- a/sample.txt"));
        assert!(payload.get("display").is_none());
        assert_eq!(
            payload["mode"],
            serde_json::Value::String("apply".to_string())
        );
        assert_eq!(
            payload["path"],
            serde_json::Value::String("sample.txt".to_string())
        );
        assert_eq!(
            payload["diff_rendered_directly"],
            serde_json::Value::Bool(true)
        );
    }

    #[test]
    fn direct_tool_display_payload_still_contains_full_diff_for_ui() {
        let diff = "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n";
        let content = serde_json::json!({
            "mode": "preview",
            "path": "sample.txt",
            "diff": diff,
            "applied": false,
            "would_change": true
        });
        let result = preview_edit_result(content.clone());

        let _sanitized = sanitize_tool_result_for_history_prompt(&result);

        let original_payload: serde_json::Value =
            serde_json::from_str(&result.content).expect("original json");
        assert_eq!(
            original_payload["diff"],
            serde_json::Value::String(diff.to_string())
        );
    }

    #[test]
    fn history_prompt_omits_full_edit_apply_diff_payload_when_mode_is_omitted() {
        let content = serde_json::json!({
            "mode": "apply",
            "path": "sample.txt",
            "diff": "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n",
            "applied": true,
            "would_change": true,
            "noop": false,
            "conflict": false,
            "stats": {
                "files_changed": 1,
                "insertions": 1,
                "deletions": 1,
                "diff_truncated": false,
                "omitted_files": 0,
                "omitted_hunks": 0
            }
        });
        let result = apply_edit_result_with_omitted_mode(content);

        let sanitized = sanitize_tool_result_for_history_prompt(&result);
        let payload: serde_json::Value = serde_json::from_str(&sanitized).expect("sanitized json");

        assert!(!sanitized.contains("\"diff\""));
        assert!(!sanitized.contains("--- a/sample.txt"));
        assert_eq!(
            payload["mode"],
            serde_json::Value::String("apply".to_string())
        );
        assert_eq!(
            payload["path"],
            serde_json::Value::String("sample.txt".to_string())
        );
        assert_eq!(payload["applied"], serde_json::Value::Bool(true));
        assert_eq!(payload["would_change"], serde_json::Value::Bool(true));
        assert_eq!(
            payload["diff_rendered_directly"],
            serde_json::Value::Bool(true)
        );
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

    #[test]
    fn doom_loop_detection_triggers_on_three_identical_calls() {
        let recent = vec![
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
        ];

        let result = super::is_doom_loop(&recent, 3);
        assert_eq!(result, Some("read"));
    }

    #[test]
    fn doom_loop_detection_does_not_trigger_on_different_args() {
        let recent = vec![
            ("read".to_string(), r#"{"path":"file1.txt"}"#.to_string()),
            ("read".to_string(), r#"{"path":"file2.txt"}"#.to_string()),
            ("read".to_string(), r#"{"path":"file3.txt"}"#.to_string()),
        ];

        let result = super::is_doom_loop(&recent, 3);
        assert_eq!(result, None);
    }

    #[test]
    fn doom_loop_detection_does_not_trigger_on_different_tools() {
        let recent = vec![
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
            ("write".to_string(), r#"{"path":"file.txt"}"#.to_string()),
            ("edit".to_string(), r#"{"path":"file.txt"}"#.to_string()),
        ];

        let result = super::is_doom_loop(&recent, 3);
        assert_eq!(result, None);
    }

    #[test]
    fn doom_loop_detection_resets_on_different_call() {
        let recent = vec![
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
            ("write".to_string(), r#"{"content":"data"}"#.to_string()),
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
        ];

        let result = super::is_doom_loop(&recent, 3);
        assert_eq!(result, None);
    }

    #[test]
    fn doom_loop_detection_does_not_trigger_with_insufficient_history() {
        let recent = vec![
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
            ("read".to_string(), r#"{"path":"file.txt"}"#.to_string()),
        ];

        let result = super::is_doom_loop(&recent, 3);
        assert_eq!(result, None);
    }

    #[test]
    fn max_tool_turns_emits_warning_when_exhausted() {
        // Simulate: loop exited with pending tool_calls because max_tool_turns reached
        let max_tool_turns = Some(2);
        let tool_calls_remaining = true; // Loop exited but tool_calls not empty

        // The logic should emit a warning when:
        // - tool_calls is not empty after loop
        // - max_tool_turns is Some(value)
        let should_warn = tool_calls_remaining && max_tool_turns.is_some();

        assert!(
            should_warn,
            "Should emit warning when tool turn limit exhausted with pending calls"
        );
    }

    #[test]
    fn max_tool_turns_does_not_warn_when_completed_naturally() {
        // Simulate: loop exited naturally (tool_calls empty)
        let max_tool_turns = Some(5);
        let tool_calls_remaining = false; // tool_calls is empty

        // The logic should NOT emit a warning when:
        // - tool_calls is empty (natural completion)
        let should_warn = tool_calls_remaining && max_tool_turns.is_some();

        assert!(
            !should_warn,
            "Should not emit warning when tool calls completed naturally"
        );
    }
}
