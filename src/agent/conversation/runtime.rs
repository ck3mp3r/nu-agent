use std::time::Duration;

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};

use crate::{
    config::Config,
    llm::LlmResponse,
    plugin::RuntimeCtx,
    session::{Message, Session, SessionStore},
    tools::{closure::ClosureRegistry, executor::ToolExecutor},
};

use crate::agent::{
    protocol::{
        cancellation::{is_llm_call_cancelled, llm_call_cancelled_error},
        contracts::{ConversationRuntime, McpUsabilityState, ProgressUi},
        event::UiEvent,
        tool_args::summarize_tool_arguments,
    },
    tools::handler::{self, McpToolRegistry, ToolSource},
};
use crate::tools::mcp::{config::McpServerConfig, runtime::McpServerLifecycle};

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

    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let mut merged_prompt = crate::agent::protocol::prompt::merge_preamble_with_prompt_and_context(
            &prompt,
            context.as_deref(),
            self.config.preamble.as_deref(),
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

        if let Some(ref mut session) = self.session {
            let user_msg = Message::new("user".to_string(), prompt.clone());
            session
                .add_message(&self.store, user_msg)
                .map_err(|e| LabeledError::new(format!("Failed to save user message: {}", e)))?;

            let assistant_msg = Message::new("assistant".to_string(), response_text.clone());
            session
                .add_message(&self.store, assistant_msg)
                .map_err(|e| LabeledError::new(format!("Failed to save assistant message: {}", e)))?;

            let _compacted = session.maybe_compact(&self.store).map_err(|e| {
                ui.emit(&UiEvent::Warning {
                    message: format!("Session compaction failed: {e}"),
                });
            });
            message_count = session.messages().len();
            compaction_count = session.compaction_count();
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
