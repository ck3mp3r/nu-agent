use std::time::Duration;

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};

use crate::{
    config::Config,
    llm::LlmResponse,
    plugin::RuntimeCtx,
    session::{CompactionInvocationMode, CompactionOutcome, Message, MessageUsage, Session, SessionStore},
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
    tools::handler::{self, McpToolRegistry, ToolSource},
};
use crate::tools::mcp::{config::McpServerConfig, runtime::McpServerLifecycle};

const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

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
    let prompt_with_agents =
        crate::agent::protocol::prompt::merge_prompt_with_context(&prompt_with_skills, agents_chain);
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
    pub mcp_tool_server_handle: Option<rig::tool::server::ToolServerHandle>,
    pub mcp_lifecycle_projection: Vec<McpServerLifecycle>,
    pub mcp_server_configs: Vec<McpServerConfig>,
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
}

fn apply_switched_config(current: &mut Config, switched: Config) {
    *current = switched;
}

impl ConversationRuntime for AgentConversationRuntime {
    fn set_mcp_server_enabled(&mut self, server_name: &str, enabled: bool) -> Result<McpUsabilityState, String> {
        if !enabled {
            self.mcp_registry.set_server_enabled(server_name, false)?;
            return Ok(McpUsabilityState::Disabled);
        }

        let Some(server_config) = self
            .mcp_server_configs
            .iter()
            .find(|server| server.name == server_name)
            .cloned()
        else {
            self.mcp_registry.set_server_enabled(server_name, false)?;
            return Ok(McpUsabilityState::Failed);
        };

        match self.runtime.block_on(crate::tools::mcp::runtime::connect_servers(
            &[McpServerConfig {
                enabled: true,
                ..server_config
            }],
            self.mcp_caller_cwd.as_deref(),
        )) {
            Ok(runtime) if runtime.has_sessions() => {
                self.mcp_registry.set_server_enabled(server_name, true)?;
                Ok(McpUsabilityState::Enabled)
            }
            Ok(_) | Err(_) => {
                self.mcp_registry.set_server_enabled(server_name, false)?;
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
        Some(policy.evaluate(Some(session.messages().len()), &mut self.auto_compaction_state))
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
                merged_prompt = format!("Previous conversation:\n{}\n\n---\n\n{}", history, merged_prompt);
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
                return Err(
                    LabeledError::new(format!("LLM call failed: {}", e.msg))
                        .with_label(e.msg, span),
                );
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

        let max_tool_turns = self.config.max_tool_turns.unwrap_or(5);
        let mut tool_turn = 0;

        while !llm_response.tool_calls.is_empty() && tool_turn < max_tool_turns {
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

            let tool_results = self.runtime.block_on(handler::handle_tool_calls(
                llm_response.tool_calls.clone(),
                &self.closure_registry,
                &self.mcp_registry,
                self.mcp_tool_server_handle.as_ref(),
                &self.tool_executor,
                &self.engine,
                span,
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
                    error_kind: result
                        .failure
                        .as_ref()
                        .map(|failure| failure.error_kind.as_str().to_string()),
                    message: result.failure.as_ref().map(|failure| failure.message.clone()),
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
                    message: result.failure.as_ref().map(|failure| failure.message.clone()),
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
                    result.content.clone(),
                ));
            }

            if let Some(ref mut session) = self.session {
                for result in &tool_results {
                    let tool_msg = Message::new(
                        "tool".to_string(),
                        persisted_tool_text_for(result),
                    )
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
                    return Err(
                        LabeledError::new(format!("LLM call failed: {}", e.msg))
                            .with_label(e.msg, span),
                    );
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
                let session = self
                    .session
                    .as_mut()
                    .expect("session checked as present");
                let user_msg = Message::new("user".to_string(), prompt.clone());
                session.add_message(&self.store, user_msg).map_err(|e| {
                    LabeledError::new(format!("Failed to save user message: {}", e))
                })?;

                let assistant_msg = persisted_assistant_message(&response_text, &llm_response.usage);
                session.add_message(&self.store, assistant_msg).map_err(|e| {
                    LabeledError::new(format!("Failed to save assistant message: {}", e))
                })?;
            }

            if let Some(CompactionTriggerDecision::Fire { source, .. }) = self.evaluate_auto_compaction()
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
            execute_compaction_persisted(session, store, |old_messages| {
                summarize_old_segment_with_llm(runtime, runtime_ctx, config, ui, old_messages)
            }, mode)
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
        if result.failure.is_none() { "done" } else { "failed" }
    )
}

fn persisted_assistant_message(content: &str, usage: &crate::llm::LlmUsage) -> Message {
    Message::new("assistant".to_string(), content.to_string()).with_usage(MessageUsage::new(
        usage.input_tokens,
        usage.output_tokens,
        usage.total_tokens,
    ))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use nu_protocol::Span;
    use tempfile::tempdir;

    use super::{
        execute_compaction_event_shared, execute_compaction_persisted, merge_runtime_prompt,
        persisted_assistant_message, COMPACTION_FAILURE_WARNING,
    };
    use crate::{
        agent::protocol::{
            compaction::CompactionTriggerSource,
            contracts::ProgressUi,
            event::UiEvent,
        },
        llm::LlmUsage,
        session::{CompactionOutcome, Message, SessionConfig, SessionStore},
    };

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
        session.add_message(&store, assistant).expect("persist message");

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
        assert!(agents_pos < skills_pos, "agents should appear before skills list");
        assert!(skills_pos < user_pos, "skills should remain before user prompt");
    }

    #[test]
    fn manual_and_auto_compaction_share_single_execution_path() {
        let mut ui = TestProgressUi::default();
        let counter = Cell::new(0usize);

        let manual = execute_compaction_event_shared(
            CompactionTriggerSource::SlashCompact,
            || {
                counter.set(counter.get() + 1);
                Ok(Some(CompactionOutcome {
                    summarized_count: 1,
                    kept_recent_count: 1,
                    summary_text: "summary".to_string(),
                }))
            },
        );
        if let Ok(event) = &manual {
            ui.emit(event);
        }
        let auto = execute_compaction_event_shared(
            CompactionTriggerSource::AutoThreshold,
            || {
                counter.set(counter.get() + 1);
                Ok(Some(CompactionOutcome {
                    summarized_count: 1,
                    kept_recent_count: 1,
                    summary_text: "summary".to_string(),
                }))
            },
        );
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
            .add_message(&store, Message::new("assistant".to_string(), "b".to_string()))
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
        let manual = execute_compaction_event_shared(
            CompactionTriggerSource::SlashCompact,
            || Err("Session compaction failed: disk full".to_string()),
        );
        let auto = execute_compaction_event_shared(
            CompactionTriggerSource::AutoThreshold,
            || Err("Session compaction failed: disk full".to_string()),
        );

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
            .add_message(&store, Message::new("assistant".to_string(), "b".to_string()))
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
            .add_message(&store, Message::new("assistant".to_string(), "b".to_string()))
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
}
