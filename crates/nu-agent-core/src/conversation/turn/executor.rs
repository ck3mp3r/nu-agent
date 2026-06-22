//! Owns the logic for executing a single agent turn: building the rig agent,
//! dispatching through the hook, handling cancel paths, and persisting results.
//!
//! Extracted from `AgentConversationRuntime::execute_turn` to give it a single
//! responsibility. `AgentConversationRuntime` constructs a `TurnExecutor` and delegates.

use std::sync::Arc;

use nu_protocol::{LabeledError, Span, Value};
use rig::memory::ConversationMemory;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::conversation::providers::{CachedProviderClient, ModelVisitor};
use crate::conversation::turn::{TurnContext, TurnResult, execute_turn, execute_turn_with_channel};
use crate::hook::permission_resolver::AsyncPermissionResolver;
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use crate::session::ConversationStore;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::types::{InMemoryConversationMemory, Message, ToolDefinition};

/// Outcome of `TurnExecutor::execute` — either a completed turn whose results
/// have been persisted and whose UI events have been emitted, or an early-exit
/// value (cancelled / error) that the caller can return directly.
pub enum TurnOutcome {
    /// The turn completed normally. The delegate should evaluate auto-compaction
    /// and then build the final `Value` using `build_response`.
    Completed,
    /// Early exit — the caller should return the contained `Value` directly.
    /// Used for cancellation paths where a minimal response is returned.
    EarlyReturn(Value),
}

/// Bundles the per-turn input parameters for [`TurnExecutor::execute`].
pub struct ExecuteInput {
    pub prompt: String,
    pub preamble: Option<String>,
    pub span: Span,
}

/// Data captured during a completed turn, used by `build_response` to construct
/// the final `Value`. Allows the response to be built after the executor's
/// borrows are released (so the delegate can run compaction in between).
pub struct TurnResponseData {
    pub text: String,
    pub usage: rig::completion::request::Usage,
    pub has_session: bool,
}

/// Groups the tool infrastructure fields always passed through to TurnContext.
pub struct ToolInfra {
    pub closure_registry: Arc<ClosureRegistry>,
    pub mcp_registry: Arc<McpToolRegistry>,
    pub tool_server_handle: rig::tool::server::ToolServerHandle,
    pub visible_tool_definitions: Vec<ToolDefinition>,
}

pub struct TurnExecutor<'a> {
    pub config: &'a Config,
    pub runtime: &'a tokio::runtime::Runtime,
    pub memory_state: &'a mut super::super::state::memory::MemoryState,
    pub tool_infra: ToolInfra,
    /// Stored after a completed turn so the delegate can extract it for response formatting.
    response_data: Option<TurnResponseData>,
}

impl<'a> TurnExecutor<'a> {
    pub fn new(
        config: &'a Config,
        runtime: &'a tokio::runtime::Runtime,
        memory_state: &'a mut super::super::state::memory::MemoryState,
        tool_infra: ToolInfra,
    ) -> Self {
        Self {
            config,
            runtime,
            memory_state,
            tool_infra,
            response_data: None,
        }
    }

    /// Extract the response data captured during `execute`. Returns `None` if
    /// `execute` was not called or did not complete normally.
    pub fn take_response_data(&mut self) -> Option<TurnResponseData> {
        self.response_data.take()
    }

    /// Execute the turn: dispatch through the provider visitor, handle cancellation
    /// paths, persist messages, and emit UI events. Returns `TurnOutcome::Completed`
    /// on success (caller should then evaluate compaction and call `build_response`),
    /// or `TurnOutcome::EarlyReturn(value)` for cancellation paths.
    ///
    /// The optional `ui_channel` is used when `InteractivePermissionResolver` is the
    /// permission resolver: the caller creates `(ui_tx, ui_rx)` first, gives a clone
    /// of `ui_tx` to the resolver, then passes the original pair here so the drain
    /// loop uses the same channel that the resolver writes `PermissionRequested` events
    /// to. Pass `None` for TTY/policy mode (the channel is created internally).
    pub fn execute<U: ProgressUi, P: AsyncPermissionResolver>(
        &mut self,
        ui: &mut U,
        input: ExecuteInput,
        cached_client: &CachedProviderClient,
        permission_resolver: P,
        final_session_id: Option<&str>,
        ui_channel: Option<(
            mpsc::UnboundedSender<UiEvent>,
            mpsc::UnboundedReceiver<UiEvent>,
        )>,
    ) -> Result<TurnOutcome, LabeledError> {
        let prompt = input.prompt;
        let preamble = input.preamble;
        let span = input.span;
        let conversation_id = if let Some(session_id) = final_session_id {
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

        let model_name = self.config.model.clone();

        let visitor_result = cached_client.with_model(
            &model_name,
            TurnVisitor {
                runtime: self.runtime,
                memory: self.memory_state.memory(),
                config: self.config,
                permission_resolver,
                closure_registry: self.tool_infra.closure_registry.clone(),
                mcp_registry: self.tool_infra.mcp_registry.clone(),
                tool_server_handle: &self.tool_infra.tool_server_handle,
                ui,
                prompt: prompt.clone(),
                conversation_id,
                preamble: preamble.clone(),
                visible_tool_definitions: std::mem::take(
                    &mut self.tool_infra.visible_tool_definitions,
                ),
                ui_channel,
            },
        );

        let turn_result = match visitor_result {
            Ok(result) => result,
            Err(e) if e.cancelled => {
                // Path A: rig hook cancelled — persist chat_history if available
                if let Some(session_id) = final_session_id
                    && let Some(ref messages) = e.messages
                {
                    if let Err(persist_err) = self
                        .memory_state
                        .conversation_store()
                        .append(session_id, messages, None)
                    {
                        log::warn!("Failed to persist cancelled turn messages: {}", persist_err);
                    }
                    if let Err(mem_err) = self.runtime.block_on(
                        self.memory_state
                            .memory_mut()
                            .append(session_id, messages.clone()),
                    ) {
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
                return Ok(TurnOutcome::EarlyReturn(crate::llm::format_response(
                    &llm_response,
                    self.config,
                    final_session_id,
                    0, // compaction_count not relevant for cancelled turns
                    span,
                )));
            }
            Err(e) => {
                return Err(
                    LabeledError::new(format!("Turn failed: {}", e.msg)).with_label(e.msg, span)
                );
            }
        };

        // Defensive fallback: if rig ever yields cancelled=true without providing chat_history.
        // Under v0.39+ semantics, cancellation always flows through PromptCancelled { chat_history }
        // (Path A above), so this block is not expected to trigger. Kept as a safety net in case
        // a future rig version changes that behaviour.
        if turn_result.cancelled
            && turn_result.messages.is_none()
            && let Some(session_id) = final_session_id
        {
            let mut cancelled_messages = vec![Message::user(prompt.clone())];
            if !turn_result.text.is_empty() {
                cancelled_messages.push(Message::assistant(turn_result.text.clone()));
            }
            if let Err(e) =
                self.memory_state
                    .conversation_store()
                    .append(session_id, &cancelled_messages, None)
            {
                log::warn!("Failed to persist cancelled turn messages (path B): {}", e);
            }
            if let Err(e) = self.runtime.block_on(
                self.memory_state
                    .memory_mut()
                    .append(session_id, cancelled_messages.clone()),
            ) {
                log::warn!(
                    "Failed to update in-memory context for cancelled turn (path B): {}",
                    e
                );
            }
        }

        // Persist new messages to conversation store if session exists
        if let Some(session_id) = final_session_id
            && let Some(ref messages) = turn_result.messages
        {
            if let Err(e) = self.memory_state.conversation_store().append(
                session_id,
                messages,
                Some(turn_result.last_total_tokens),
            ) {
                log::warn!(
                    "Failed to persist turn messages to conversation store: {}",
                    e
                );
            }

            // Update last_total_tokens for compaction
            *self.memory_state.last_total_tokens_mut() = Some(turn_result.last_total_tokens);
        }

        // Emit UI events
        if !turn_result.deltas_emitted {
            ui.emit(&UiEvent::AssistantMessage {
                text: turn_result.text.clone(),
            });
        }
        ui.emit(&UiEvent::Completed {
            tool_calls: turn_result.tool_call_count,
        });
        ui.flush();

        // Store turn data for response building
        self.response_data = Some(TurnResponseData {
            text: turn_result.text,
            usage: turn_result.usage,
            has_session: final_session_id.is_some(),
        });

        Ok(TurnOutcome::Completed)
    }
}

// ---------------------------------------------------------------------------
// TurnVisitor — module-level so it can be generic over P: AsyncPermissionResolver
// ---------------------------------------------------------------------------

struct TurnVisitor<'a, 'b, U, P> {
    runtime: &'a tokio::runtime::Runtime,
    memory: &'b InMemoryConversationMemory,
    config: &'a Config,
    permission_resolver: P,
    closure_registry: Arc<ClosureRegistry>,
    mcp_registry: Arc<McpToolRegistry>,
    tool_server_handle: &'a rig::tool::server::ToolServerHandle,
    ui: &'a mut U,
    prompt: String,
    conversation_id: String,
    preamble: Option<String>,
    visible_tool_definitions: Vec<ToolDefinition>,
    /// Pre-built channel for interactive (TUI) mode so the resolver's `ui_tx`
    /// and the drain loop's `ui_rx` share the same tokio unbounded channel.
    /// `None` for TTY/policy mode (channel created internally by `execute_turn`).
    ui_channel: Option<(
        mpsc::UnboundedSender<UiEvent>,
        mpsc::UnboundedReceiver<UiEvent>,
    )>,
}

impl<U: ProgressUi, P: AsyncPermissionResolver> ModelVisitor for TurnVisitor<'_, '_, U, P> {
    type Output = Result<TurnResult, crate::conversation::turn::TurnError>;

    fn visit<M>(self, model: M) -> Self::Output
    where
        M: rig::completion::CompletionModel + Clone + 'static,
    {
        let ctx = TurnContext::new(
            self.runtime.handle(),
            model,
            super::TurnConversation {
                memory: self.memory.clone(),
                conversation_id: self.conversation_id,
            },
            super::TurnInput {
                prompt: self.prompt,
                preamble: self.preamble.as_deref(),
                max_turns: self.config.max_tool_turns,
            },
            ToolInfra {
                closure_registry: self.closure_registry.clone(),
                mcp_registry: self.mcp_registry.clone(),
                tool_server_handle: self.tool_server_handle.clone(),
                visible_tool_definitions: self.visible_tool_definitions,
            },
        );
        if let Some((ui_tx, ui_rx)) = self.ui_channel {
            execute_turn_with_channel(ctx, self.ui, self.permission_resolver, ui_tx, ui_rx)
        } else {
            execute_turn(ctx, self.ui, self.permission_resolver)
        }
    }
}

/// Build the final response `Value` from turn data. Called by the delegate after
/// auto-compaction has been evaluated, so `compaction_count` reflects the current
/// post-compaction value.
pub fn build_response(
    response_data: Option<TurnResponseData>,
    config: &Config,
    session_id: Option<&str>,
    compaction_count: usize,
    span: Span,
) -> Value {
    let data = response_data.unwrap_or(TurnResponseData {
        text: String::new(),
        usage: rig::completion::request::Usage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            tool_use_prompt_tokens: 0,
            reasoning_tokens: 0,
        },
        has_session: false,
    });

    let message_count = 0;

    let llm_response = crate::llm::LlmResponse {
        text: data.text,
        usage: crate::llm::LlmUsage {
            input_tokens: data.usage.input_tokens,
            output_tokens: data.usage.output_tokens,
            total_tokens: data.usage.total_tokens,
            cached_input_tokens: data.usage.cached_input_tokens,
            cache_creation_input_tokens: data.usage.cache_creation_input_tokens,
        },
        tool_calls: Vec::new(),         // TODO: track tool calls in TurnResult
        tool_call_metadata: Vec::new(), // TODO: track tool metadata in TurnResult
    };

    let response_value =
        crate::llm::format_response(&llm_response, config, session_id, compaction_count, span);

    if data.has_session
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
            return Value::record(new_record, span);
        }
    }

    response_value
}

#[cfg(test)]
#[path = "executor_test.rs"]
mod executor_test;
