//! Owns the logic for executing a single agent turn: building the rig agent,
//! dispatching through the hook, handling cancel paths, and persisting results.
//!
//! Extracted from `AgentConversationRuntime::execute_turn` to give it a single
//! responsibility. `AgentConversationRuntime` constructs a `TurnExecutor` and delegates.

use nu_protocol::{LabeledError, Span, Value};
use rig::memory::ConversationMemory;

use crate::agent::conversation::providers::{CachedProviderClient, ModelVisitor};
use crate::agent::conversation::turn::{TurnContext, TurnResult, execute_turn};
use crate::agent::hook::AuthzPermissionResolver;
use crate::agent::protocol::contracts::ProgressUi;
use crate::agent::protocol::event::UiEvent;
use crate::agent::tools::authz::{AsyncAskHook, PermissionsConfig, SessionGrantCache};
use crate::agent::tools::handler::McpToolRegistry;
use crate::config::Config;
use crate::session::{ConversationStore, JsonlConversationStore};
use crate::tools::closure::ClosureRegistry;
use crate::types::{InMemoryConversationMemory, Message, ToolDefinition};

use nu_plugin::EngineInterface;

/// Outcome of `TurnExecutor::execute` — either a completed turn whose results
/// have been persisted and whose UI events have been emitted, or an early-exit
/// value (cancelled / error) that the caller can return directly.
pub(crate) enum TurnOutcome {
    /// The turn completed normally. The delegate should evaluate auto-compaction
    /// and then build the final `Value` using `build_response`.
    Completed,
    /// Early exit — the caller should return the contained `Value` directly.
    /// Used for cancellation paths where a minimal response is returned.
    EarlyReturn(Value),
}

/// Bundles the per-turn input parameters for [`TurnExecutor::execute`].
pub(crate) struct ExecuteInput {
    pub(crate) prompt: String,
    pub(crate) preamble: Option<String>,
    pub(crate) span: Span,
}

/// Data captured during a completed turn, used by `build_response` to construct
/// the final `Value`. Allows the response to be built after the executor's
/// borrows are released (so the delegate can run compaction in between).
pub(crate) struct TurnResponseData {
    pub(crate) text: String,
    pub(crate) usage: rig::completion::request::Usage,
    pub(crate) has_session: bool,
}

/// Groups the 3 permission-related fields always passed together to build AuthzPermissionResolver.
pub(crate) struct PermissionCtx<'a> {
    pub(crate) permissions: &'a PermissionsConfig,
    pub(crate) session_grants: &'a mut SessionGrantCache,
    pub(crate) ask_hook: &'a mut AsyncAskHook,
}

/// Groups the 4 conversation persistence fields always used together in the persist block.
pub(crate) struct ConversationState<'a> {
    pub(crate) memory: &'a mut InMemoryConversationMemory,
    pub(crate) conversation_store: &'a JsonlConversationStore,
    pub(crate) last_total_tokens: &'a mut Option<u64>,
    pub(crate) final_session_id: &'a Option<String>,
}

/// Groups the 3 tool infrastructure fields always passed through to TurnContext.
pub(crate) struct ToolInfra<'a> {
    pub(crate) closure_registry: &'a ClosureRegistry,
    pub(crate) mcp_registry: &'a McpToolRegistry,
    pub(crate) mcp_tool_server_handle: &'a rig::tool::server::ToolServerHandle,
}

pub(crate) struct TurnExecutor<'a> {
    pub(crate) config: &'a Config,
    pub(crate) runtime: &'a tokio::runtime::Runtime,
    pub(crate) permission_ctx: PermissionCtx<'a>,
    pub(crate) conversation_state: ConversationState<'a>,
    pub(crate) tool_infra: ToolInfra<'a>,
    /// Stored after a completed turn so the delegate can extract it for response formatting.
    response_data: Option<TurnResponseData>,
}

impl<'a> TurnExecutor<'a> {
    pub(crate) fn new(
        config: &'a Config,
        runtime: &'a tokio::runtime::Runtime,
        permission_ctx: PermissionCtx<'a>,
        conversation_state: ConversationState<'a>,
        tool_infra: ToolInfra<'a>,
    ) -> Self {
        Self {
            config,
            runtime,
            permission_ctx,
            conversation_state,
            tool_infra,
            response_data: None,
        }
    }

    /// Extract the response data captured during `execute`. Returns `None` if
    /// `execute` was not called or did not complete normally.
    pub(crate) fn take_response_data(&mut self) -> Option<TurnResponseData> {
        self.response_data.take()
    }

    /// Execute the turn: dispatch through the provider visitor, handle cancellation
    /// paths, persist messages, and emit UI events. Returns `TurnOutcome::Completed`
    /// on success (caller should then evaluate compaction and call `build_response`),
    /// or `TurnOutcome::EarlyReturn(value)` for cancellation paths.
    pub(crate) fn execute<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        input: ExecuteInput,
        cached_client: &CachedProviderClient,
        visible_tool_definitions: Vec<ToolDefinition>,
        engine: &EngineInterface,
    ) -> Result<TurnOutcome, LabeledError> {
        let prompt = input.prompt;
        let preamble = input.preamble;
        let span = input.span;
        let conversation_id = if let Some(session_id) = self.conversation_state.final_session_id {
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

        // Visitor that executes a conversation turn with the provider's completion model.
        struct TurnVisitor<'a, 'b, U> {
            runtime: &'a tokio::runtime::Runtime,
            memory: &'b InMemoryConversationMemory,
            config: &'a Config,
            permissions: &'a PermissionsConfig,
            session_grants: &'a mut SessionGrantCache,
            ask_hook: &'a mut AsyncAskHook,
            engine: &'a EngineInterface,
            closure_registry: &'a ClosureRegistry,
            mcp_registry: &'a McpToolRegistry,
            mcp_tool_server_handle: &'a rig::tool::server::ToolServerHandle,
            ui: &'a mut U,
            prompt: String,
            conversation_id: String,
            preamble: Option<String>,
            visible_tool_definitions: Vec<ToolDefinition>,
        }

        impl<U: ProgressUi> ModelVisitor for TurnVisitor<'_, '_, U> {
            type Output = Result<TurnResult, crate::agent::conversation::turn::TurnError>;

            fn visit<M>(self, model: M) -> Self::Output
            where
                M: rig::completion::CompletionModel + Clone + 'static,
            {
                let mut permission_resolver = AuthzPermissionResolver {
                    permissions: self.permissions,
                    grant_cache: self.session_grants,
                    ask_hook: self.ask_hook,
                    engine: self.engine,
                    closure_registry: self.closure_registry,
                    mcp_registry: self.mcp_registry,
                };
                execute_turn(
                    TurnContext {
                        runtime: self.runtime.handle(),
                        model,
                        prompt: self.prompt,
                        memory: self.memory.clone(),
                        conversation_id: self.conversation_id,
                        preamble: self.preamble.as_deref(),
                        max_turns: self.config.max_tool_turns,
                        tool_server_handle: self.mcp_tool_server_handle.clone(),
                        visible_tool_definitions: self.visible_tool_definitions,
                        closure_registry: self.closure_registry,
                        mcp_registry: self.mcp_registry,
                    },
                    self.ui,
                    &mut permission_resolver,
                )
            }
        }

        let visitor_result = cached_client.with_model(
            &model_name,
            TurnVisitor {
                runtime: self.runtime,
                memory: self.conversation_state.memory,
                config: self.config,
                permissions: self.permission_ctx.permissions,
                session_grants: self.permission_ctx.session_grants,
                ask_hook: self.permission_ctx.ask_hook,
                engine,
                closure_registry: self.tool_infra.closure_registry,
                mcp_registry: self.tool_infra.mcp_registry,
                mcp_tool_server_handle: self.tool_infra.mcp_tool_server_handle,
                ui,
                prompt: prompt.clone(),
                conversation_id,
                preamble: preamble.clone(),
                visible_tool_definitions,
            },
        );

        let turn_result = match visitor_result {
            Ok(result) => result,
            Err(e) if e.cancelled => {
                // Path A: rig hook cancelled — persist chat_history if available
                if let Some(session_id) = self.conversation_state.final_session_id
                    && let Some(ref messages) = e.messages
                {
                    if let Err(persist_err) = self
                        .conversation_state
                        .conversation_store
                        .append(session_id, messages, None)
                    {
                        log::warn!("Failed to persist cancelled turn messages: {}", persist_err);
                    }
                    if let Err(mem_err) = self.runtime.block_on(
                        self.conversation_state
                            .memory
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
                    self.conversation_state.final_session_id.as_deref(),
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

        // Path B: cancel_token fired — FinalResponse never arrived so messages is None.
        // Construct user + optional partial assistant message and persist manually.
        if turn_result.cancelled
            && turn_result.messages.is_none()
            && let Some(session_id) = self.conversation_state.final_session_id
        {
            let mut cancelled_messages = vec![Message::user(prompt.clone())];
            if !turn_result.text.is_empty() {
                cancelled_messages.push(Message::assistant(turn_result.text.clone()));
            }
            if let Err(e) = self.conversation_state.conversation_store.append(
                session_id,
                &cancelled_messages,
                None,
            ) {
                log::warn!("Failed to persist cancelled turn messages (path B): {}", e);
            }
            if let Err(e) = self.runtime.block_on(
                self.conversation_state
                    .memory
                    .append(session_id, cancelled_messages.clone()),
            ) {
                log::warn!(
                    "Failed to update in-memory context for cancelled turn (path B): {}",
                    e
                );
            }
        }

        // Persist new messages to conversation store if session exists
        if let Some(session_id) = self.conversation_state.final_session_id
            && let Some(ref messages) = turn_result.messages
        {
            if let Err(e) = self.conversation_state.conversation_store.append(
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
            *self.conversation_state.last_total_tokens = Some(turn_result.last_total_tokens);
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
            has_session: self.conversation_state.final_session_id.is_some(),
        });

        Ok(TurnOutcome::Completed)
    }
}

/// Build the final response `Value` from turn data. Called by the delegate after
/// auto-compaction has been evaluated, so `compaction_count` reflects the current
/// post-compaction value.
pub(crate) fn build_response(
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
#[path = "turn_executor_test.rs"]
mod turn_executor_test;
