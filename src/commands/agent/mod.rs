use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, LabeledError, Signature, Type, Value};
use std::io::IsTerminal;
use std::time::Duration;

use crate::{
    AgentPlugin,
    config::{Config, PluginConfig},
    plugin::RuntimeCtx,
};

mod tool_handler;
mod ui;

use self::ui::{
    event::UiEvent,
    factory::{StderrUiFactory, UiRendererFactory},
    policy::resolve_ui_policy,
    renderer::UiRenderer,
};

enum LlmCallProgress {
    Tick,
    Done(Result<crate::llm::LlmResponse, LabeledError>),
}

fn call_llm_with_ui_ticks<R: UiRenderer>(
    runtime: &tokio::runtime::Runtime,
    runtime_ctx: &RuntimeCtx,
    config: &Config,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
    ui_renderer: &mut R,
) -> Result<crate::llm::LlmResponse, LabeledError> {
    let mut call_fut = std::pin::pin!(crate::llm::call_llm(runtime_ctx, config, prompt, tools));

    loop {
        match runtime.block_on(async {
            tokio::select! {
                response = &mut call_fut => LlmCallProgress::Done(response),
                _ = tokio::time::sleep(Duration::from_millis(80)) => LlmCallProgress::Tick,
            }
        }) {
            LlmCallProgress::Tick => ui_renderer.emit(&UiEvent::Tick),
            LlmCallProgress::Done(result) => return result,
        }
    }
}

/// Trait abstracting the engine interface functionality needed for config resolution.
///
/// This allows us to mock the EngineInterface for testing without needing
/// a real Nushell engine instance.
pub trait EngineConfigInterface {
    fn get_plugin_config(&self) -> Result<Option<Value>, LabeledError>;
}

impl EngineConfigInterface for EngineInterface {
    fn get_plugin_config(&self) -> Result<Option<Value>, LabeledError> {
        // Convert ShellError to LabeledError
        self.get_plugin_config()
            .map_err(|e| LabeledError::new(format!("Failed to get plugin config: {}", e)))
    }
}

/// Extract prompt string from input Value.
///
/// Supports two input formats:
/// 1. String input: "prompt text"
/// 2. Record input: {prompt: "prompt text", context?: "...", model?: "...", tools?: [...]}
///
/// # Arguments
/// * `input` - The input Value, expected to be a String or Record with 'prompt' field
///
/// # Returns
/// The prompt string, or error if input is invalid
///
/// # Errors
/// - Input is not a String or Record
/// - Record input missing 'prompt' field
/// - Prompt is empty or contains only whitespace
pub fn extract_prompt_from_input(input: &Value) -> Result<String, LabeledError> {
    // Try to extract as string first (original behavior)
    if let Ok(prompt_str) = input.as_str() {
        // Check for empty string
        if prompt_str.trim().is_empty() {
            return Err(LabeledError::new("Empty prompt")
                .with_label("Prompt cannot be empty", input.span()));
        }
        return Ok(prompt_str.to_string());
    }

    // Try to extract as record
    if let Ok(record) = input.as_record() {
        // Look for 'prompt' field
        let prompt_value = record.get("prompt").ok_or_else(|| {
            LabeledError::new("Missing required field")
                .with_label("Record input must have 'prompt' field", input.span())
        })?;

        // Extract string from prompt field
        let prompt_str = prompt_value.as_str().map_err(|_| {
            LabeledError::new("Invalid prompt type")
                .with_label("'prompt' field must be a string", prompt_value.span())
        })?;

        // Check for empty string
        if prompt_str.trim().is_empty() {
            return Err(LabeledError::new("Empty prompt")
                .with_label("Prompt cannot be empty", prompt_value.span()));
        }

        return Ok(prompt_str.to_string());
    }

    // Neither string nor record - error
    Err(LabeledError::new("Invalid input type").with_label(
        "Expected a string prompt or record with 'prompt' field",
        input.span(),
    ))
}

/// Extract optional context string from input Value.
///
/// Supports two input formats:
/// 1. String input: Returns None (no context field available)
/// 2. Record input: Returns Some(context) if 'context' field exists, None otherwise
///
/// # Arguments
/// * `input` - The input Value
///
/// # Returns
/// Optional context string, or error if context field has invalid type
///
/// # Errors
/// - Context field exists but is not a string
pub fn extract_context_from_input(input: &Value) -> Result<Option<String>, LabeledError> {
    // String input has no context field
    if input.as_str().is_ok() {
        return Ok(None);
    }

    // Try to extract as record
    if let Ok(record) = input.as_record() {
        // Look for optional 'context' field
        if let Some(context_value) = record.get("context") {
            // Extract string from context field
            let context_str = context_value.as_str().map_err(|_| {
                LabeledError::new("Invalid context type")
                    .with_label("'context' field must be a string", context_value.span())
            })?;

            return Ok(Some(context_str.to_string()));
        }

        // No context field - that's OK
        return Ok(None);
    }

    // Neither string nor record - no context
    Ok(None)
}

/// Merge optional context with prompt for LLM call.
///
/// If context is provided and non-empty, prepends it to the prompt with clear separation.
/// Empty or whitespace-only context is treated as None.
///
/// # Arguments
/// * `prompt` - The main prompt text
/// * `context` - Optional context to prepend to the prompt
///
/// # Returns
/// Combined prompt string with context prepended if provided
pub fn merge_prompt_with_context(prompt: &str, context: Option<&str>) -> String {
    match context {
        Some(ctx) if !ctx.trim().is_empty() => {
            format!("{}\n\n---\n\n{}", ctx, prompt)
        }
        _ => prompt.to_string(),
    }
}

/// Extracts and validates session flags from the evaluated call.
///
/// Returns a tuple of (session_id, new_session, no_session).
/// Validates that flags are mutually exclusive.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing session flags
///
/// # Returns
/// A tuple of (`Option<String>`, bool, bool) representing the session flags.
///
/// # Errors
/// Returns an error if:
/// - Multiple session flags are provided together
pub fn extract_and_validate_session_flags(
    call: &EvaluatedCall,
) -> Result<(Option<String>, bool), LabeledError> {
    // Extract flags
    let session_id = call.get_flag::<String>("session").ok().flatten();
    let new_session = call.has_flag("new-session")?;

    // Validate mutual exclusion: can't use both --session and --new-session
    if session_id.is_some() && new_session {
        return Err(LabeledError::new("Conflicting session flags").with_label(
            "Cannot use both --session and --new-session together",
            call.head,
        ));
    }

    Ok((session_id, new_session))
}

/// Extract and parse closures from --tools flag.
///
/// Returns a HashMap of tool name to `Spanned<Closure>`, filtering out any non-closure values.
/// If the flag is not provided or is not a record, returns an empty HashMap.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing the --tools flag
///
/// # Returns
/// HashMap of tool names to spanned closures
pub fn extract_tools_from_call(
    call: &EvaluatedCall,
) -> Result<
    std::collections::HashMap<String, nu_protocol::Spanned<nu_protocol::engine::Closure>>,
    LabeledError,
> {
    use std::collections::HashMap;

    // Try to get --tools flag
    let tools_value: Option<Value> = call.get_flag("tools").ok().flatten();

    match tools_value {
        Some(Value::Record { val, .. }) => {
            // Filter and extract closures from the record
            let closures = val
                .iter()
                .filter_map(|(name, value)| {
                    if let Value::Closure {
                        val, internal_span, ..
                    } = value
                    {
                        // val is a Box<Closure>, need to deref and clone
                        // Wrap with span to preserve source location
                        Some((
                            name.to_string(),
                            nu_protocol::Spanned {
                                item: (**val).clone(),
                                span: *internal_span,
                            },
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            Ok(closures)
        }
        Some(_) => {
            // Non-record value provided - return empty HashMap (graceful handling)
            Ok(HashMap::new())
        }
        None => {
            // Flag not provided - return empty HashMap
            Ok(HashMap::new())
        }
    }
}

/// Extract and parse --tool-timeout flag.
///
/// Returns a Duration parsed from Nushell duration value (i64 nanoseconds).
/// If the flag is not provided, returns default of 30 seconds.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing the --tool-timeout flag
///
/// # Returns
/// Duration for tool execution timeout
pub fn extract_tool_timeout(call: &EvaluatedCall) -> std::time::Duration {
    // Extract the flag value (i64 nanoseconds)
    let timeout_nanos: Option<i64> = call.get_flag("tool-timeout").ok().flatten();

    // Convert to Duration, defaulting to 30 seconds
    timeout_nanos
        .map(|nanos| std::time::Duration::from_nanos(nanos as u64))
        .unwrap_or(std::time::Duration::from_secs(30))
}

/// Extract MCP tool name patterns from --mcp-tools flag.
///
/// Expected input is a list of strings, e.g. ["k8s__*", "gh__list_*"]
///
/// Returns an empty vector when the flag is not provided.
/// Empty vector means "no filtering" (match all MCP tools).
pub fn extract_mcp_patterns_from_call(call: &EvaluatedCall) -> Result<Vec<String>, LabeledError> {
    let patterns_value: Option<Value> = call.get_flag("mcp-tools").ok().flatten();

    let Some(value) = patterns_value else {
        return Ok(Vec::new());
    };

    let list = value.as_list().map_err(|_| {
        LabeledError::new("Invalid --mcp-tools value")
            .with_label("--mcp-tools must be a list of strings", value.span())
    })?;

    let mut patterns = Vec::with_capacity(list.len());
    for item in list {
        let pattern = item.as_str().map_err(|_| {
            LabeledError::new("Invalid --mcp-tools entry")
                .with_label("Each --mcp-tools entry must be a string", item.span())
        })?;
        patterns.push(pattern.to_string());
    }

    Ok(patterns)
}

/// Select MCP tools from config, optionally intersected by CLI allowlist patterns.
///
/// Behavior:
/// - No config => empty set
/// - Empty patterns => all runtime-discovered MCP tools
/// - Non-empty patterns => only runtime-discovered tools matching patterns
pub fn select_mcp_tools(
    discovered_tools: &[crate::tools::mcp::client::McpToolDefinition],
    cli_allowlist_patterns: &[String],
) -> Vec<crate::tools::mcp::client::McpToolDefinition> {
    crate::tools::mcp::registration::registerable_tools(discovered_tools, cli_allowlist_patterns)
}

pub struct Agent {
    store: crate::session::SessionStore,
    runtime_ctx: RuntimeCtx,
}

impl Agent {
    /// Creates a new Agent command with the given SessionStore.
    pub fn new(store: crate::session::SessionStore, runtime_ctx: RuntimeCtx) -> Self {
        Self { store, runtime_ctx }
    }
}

impl SimplePluginCommand for Agent {
    type Plugin = AgentPlugin;

    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "Send a prompt to an AI agent and get a structured response"
    }

    fn signature(&self) -> Signature {
        Signature::build(PluginCommand::name(self))
            .input_output_types(vec![
                (Type::String, Type::Record(vec![].into())),
                (Type::Record(vec![].into()), Type::Record(vec![].into())),
            ])
            .category(Category::Experimental)
            .named(
                "provider",
                nu_protocol::SyntaxShape::String,
                "[DEPRECATED] LLM provider name - use --model with provider/model format instead",
                Some('p'),
            )
            .named(
                "model",
                nu_protocol::SyntaxShape::String,
                "Model to use in provider/model format (e.g., 'openai/gpt-4', 'anthropic/claude-3-opus')",
                Some('m'),
            )
            .switch(
                "small",
                "Use the small/fast model configured in plugin config",
                Some('s'),
            )
            .named(
                "api-key",
                nu_protocol::SyntaxShape::String,
                "API key override for the provider",
                None,
            )
            .named(
                "base-url",
                nu_protocol::SyntaxShape::String,
                "Custom API endpoint URL",
                None,
            )
            .named(
                "temperature",
                nu_protocol::SyntaxShape::Number,
                "Sampling temperature (0.0 to 2.0)",
                None,
            )
            .named(
                "max-tokens",
                nu_protocol::SyntaxShape::Int,
                "Maximum tokens to generate",
                None,
            )
            .named(
                "max-context-tokens",
                nu_protocol::SyntaxShape::Int,
                "Maximum context window size (input + output)",
                None,
            )
            .named(
                "max-output-tokens",
                nu_protocol::SyntaxShape::Int,
                "Maximum output tokens",
                None,
            )
            .named(
                "max-turns",
                nu_protocol::SyntaxShape::Int,
                "Maximum tool calling turns",
                None,
            )
            .named(
                "tools",
                nu_protocol::SyntaxShape::Record(vec![]),
                "Record of tool closures: {name: closure, ...}",
                None,
            )
            .named(
                "mcp-tools",
                nu_protocol::SyntaxShape::List(Box::new(nu_protocol::SyntaxShape::String)),
                "List of MCP tool name glob patterns, e.g. ['k8s__*', 'gh__list_*']",
                None,
            )
            .named(
                "tool-timeout",
                nu_protocol::SyntaxShape::Duration,
                "Timeout for tool execution (default: 30sec)",
                Some('t'),
            )
            .named(
                "session",
                nu_protocol::SyntaxShape::String,
                "Session ID to use (auto-creates if doesn't exist)",
                None,
            )
            .switch(
                "new-session",
                "Create new session with auto-generated ID",
                None,
            )
            .switch(
                "verbose",
                "Increase UX detail; repeat for more detail (-v, -vv, -vvv)",
                Some('v'),
            )
            .switch(
                "quiet",
                "Suppress non-essential UX progress output",
                Some('q'),
            )
    }

    fn run(
        &self,
        _plugin: &AgentPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        let ui_policy = resolve_ui_policy(call)
            .map_err(|e| LabeledError::new(format!("Failed to resolve UI policy: {e}")))?;
        let stderr_is_tty = std::io::stderr().is_terminal();
        let mut ui_renderer = StderrUiFactory::new(std::io::stderr(), stderr_is_tty).create(ui_policy);

        // Validate session flags
        let (session_id, new_session) = extract_and_validate_session_flags(call)?;

        // Resolve configuration from all sources with proper precedence:
        // default < env < plugin < flags
        let config = resolve_config(engine, call)?;

        // Extract tool timeout for ToolExecutor
        let tool_timeout = extract_tool_timeout(call);

        // Extract tools from --tools flag and build ClosureRegistry
        let tools_map = extract_tools_from_call(call)?;
        let mut closure_registry = crate::tools::closure::ClosureRegistry::new();
        for (name, closure) in &tools_map {
            closure_registry.register(name.clone(), closure.clone());
        }

        // Extract optional MCP tool name patterns.
        // Empty patterns means "no filtering" (match all MCP tools).
        let mcp_patterns = extract_mcp_patterns_from_call(call)?;

        let mcp_config = engine
            .get_plugin_config()?
            .map(|value| crate::tools::mcp::config::McpConfig::from_plugin_config(&value))
            .transpose()
            .map_err(|err| {
                LabeledError::new("Failed to load MCP config")
                    .with_label(err.to_string(), call.head)
            })?;

        // Create async runtime for LLM and MCP tool execution
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| LabeledError::new(format!("Failed to create async runtime: {}", e)))?;

        let mcp_runtime = if let Some(cfg) = mcp_config.as_ref() {
            if cfg.mcp.is_empty() {
                None
            } else {
                let caller_cwd = engine.get_current_dir().map_err(|e| {
                    LabeledError::new("Failed to resolve caller cwd")
                        .with_label(format!("Unable to read current dir from Nushell engine: {e}"), call.head)
                })?;
                let caller_cwd_path = std::path::Path::new(&caller_cwd);

                Some(
                    runtime
                        .block_on(crate::tools::mcp::runtime::connect_servers(
                            &cfg.mcp,
                            Some(caller_cwd_path),
                        ))
                        .map_err(|msg| {
                            LabeledError::new("Failed to connect MCP runtime")
                                .with_label(msg, call.head)
                        })?,
                )
            }
        } else {
            None
        };

        let discovered_mcp_tools = if let Some(mcp_runtime) = mcp_runtime.as_ref() {
            select_mcp_tools(mcp_runtime.discovered_tools(), &mcp_patterns)
        } else {
            Vec::new()
        };

        let mcp_tool_server_handle = mcp_runtime.as_ref().map(|r| r.tool_server_handle());

        let mcp_registry = crate::commands::agent::tool_handler::McpToolRegistry::from_tools(
            discovered_mcp_tools.clone(),
        )
        .map_err(|msg| {
            LabeledError::new("Failed to build MCP tool registry").with_label(msg, call.head)
        })?;

        // Convert closures to tool definitions for LLM
        use crate::tools::closure::closure_to_tool_definition;
        let mut tool_definitions: Vec<rig::completion::ToolDefinition> = tools_map
            .iter()
            .map(|(name, closure)| closure_to_tool_definition(name.clone(), closure, engine, None))
            .collect();

        tool_definitions.extend(discovered_mcp_tools.iter().map(|tool| {
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
        }));

        // Extract prompt from input
        let prompt = extract_prompt_from_input(input)?;

        // Extract optional context from input
        let context = extract_context_from_input(input)?;

        // Determine if we should use a session
        let use_session = session_id.is_some() || new_session;
        let mut session_opt = None;
        let mut final_session_id = None;

        // Load or create session if requested
        if use_session {
            let id = if let Some(id) = session_id {
                id
            } else if new_session {
                // Generate auto session ID: session-YYYYMMDD-HHMMSS-micros
                use chrono::Utc;
                let now = Utc::now();
                format!(
                    "session-{}-{}",
                    now.format("%Y%m%d-%H%M%S"),
                    now.timestamp_subsec_micros()
                )
            } else {
                unreachable!()
            };

            // Load or create the session
            let session = self
                .store
                .get_or_create(Some(id.clone()))
                .map_err(|e| LabeledError::new(format!("Failed to load/create session: {}", e)))?;

            final_session_id = Some(id.clone());
            session_opt = Some(session);
        }

        // Build prompt with session history if available
        let mut merged_prompt = merge_prompt_with_context(&prompt, context.as_deref());

        if let Some(ref session) = session_opt {
            let history = session.format_history();

            if !history.is_empty() {
                merged_prompt = format!(
                    "Previous conversation:\n{}\n\n---\n\n{}",
                    history, merged_prompt
                );
            }
        }

        // Call LLM and handle tool execution loop
        ui_renderer.emit(&UiEvent::LlmStart);
        let mut llm_response = call_llm_with_ui_ticks(
            &runtime,
            &self.runtime_ctx,
            &config,
            &merged_prompt,
            tool_definitions.clone(),
            &mut ui_renderer,
        )
            .map_err(|e| {
                LabeledError::new(format!("LLM call failed: {}", e.msg))
                    .with_label(e.msg, call.head)
            })?;
        ui_renderer.emit(&UiEvent::LlmEnd {
            response_chars: llm_response.text.len(),
            tool_calls: llm_response.tool_calls.len(),
        });

        // Track all tool calls executed during the agent loop
        let mut executed_tool_calls: Vec<rig::completion::AssistantContent> = Vec::new();
        let mut tool_results_metadata: Vec<crate::llm::ToolCallMetadata> = Vec::new();

        // Create audit log directory ONCE before agent loop
        // This follows the Single Responsibility Principle: the logger only logs,
        // the caller is responsible for ensuring the directory exists
        let log_dir = crate::utils::xdg::data_dir()
            .map_err(|e| LabeledError::new(format!("XDG data directory error: {}", e)))?
            .join("nu-agent");
        std::fs::create_dir_all(&log_dir).map_err(|e| {
            LabeledError::new(format!("Failed to create audit log directory: {}", e))
        })?;
        let log_path = log_dir.join("tool_audit.log");

        // Create AuditLogger ONCE with pre-existing directory
        let audit_logger = std::sync::Arc::new(crate::tools::audit::AuditLogger::new(log_path));

        // Create ToolExecutor ONCE with engine, audit logger, and timeout
        let tool_executor = crate::tools::executor::ToolExecutor::new(
            std::sync::Arc::new(engine.clone()),
            audit_logger,
            tool_timeout,
        );

        // In-memory conversation tracking (works with or without session)
        // This ensures tool results are ALWAYS passed to subsequent LLM calls
        // Session tracking is SEPARATE (optional persistence to disk)
        let mut conversation_messages: Vec<(String, String)> = vec![];

        // Track initial user prompt and first assistant response
        conversation_messages.push(("user".to_string(), merged_prompt.clone()));
        conversation_messages.push(("assistant".to_string(), llm_response.text.clone()));

        // Agent loop: process tool calls if present
        let max_tool_turns = config.max_tool_turns.unwrap_or(5);
        let mut tool_turn = 0;

        while !llm_response.tool_calls.is_empty() && tool_turn < max_tool_turns {
            tool_turn += 1;

            // Log tool calls before execution
            for content in &llm_response.tool_calls {
                if let rig::completion::message::AssistantContent::ToolCall(tc) = content {
                    let source = if closure_registry.get(&tc.function.name).is_some() {
                        "closure".to_string()
                    } else if mcp_registry.contains(&tc.function.name) {
                        "mcp".to_string()
                    } else {
                        "unknown".to_string()
                    };
                    ui_renderer.emit(&UiEvent::ToolStart {
                        name: tc.function.name.clone(),
                        source,
                        arguments: serde_json::to_string(&tc.function.arguments)
                            .unwrap_or_else(|_| "{}".to_string()),
                    });
                }
            }

            // Capture tool calls that were executed this turn
            executed_tool_calls.extend(llm_response.tool_calls.clone());

            // Execute tool calls
            let tool_results = runtime.block_on(tool_handler::handle_tool_calls(
                llm_response.tool_calls.clone(),
                &closure_registry,
                &mcp_registry,
                mcp_tool_server_handle.as_ref(),
                &tool_executor,
                engine,
                call.head,
            ));

            // Log tool results
            for result in &tool_results {
                let source = match result.source {
                    crate::commands::agent::tool_handler::ToolSource::Closure => {
                        "closure".to_string()
                    }
                    crate::commands::agent::tool_handler::ToolSource::Mcp => "mcp".to_string(),
                    crate::commands::agent::tool_handler::ToolSource::Unknown => {
                        "unknown".to_string()
                    }
                };

                ui_renderer.emit(&UiEvent::ToolEnd {
                    name: result.tool_name.clone(),
                    source: source.clone(),
                    arguments: result.arguments.clone(),
                    success: result.failure.is_none(),
                    result: result.content.clone(),
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

            // Track tool results in-memory conversation (ALWAYS, regardless of session)
            for result in &tool_results {
                conversation_messages.push((
                    "tool".to_string(),
                    format!(
                        "Tool '{}' returned: {}",
                        result.tool_call_id, result.content
                    ),
                ));
            }

            // Save tool results to session if active (SEPARATE from in-memory tracking)
            if let Some(ref mut session) = session_opt {
                for result in &tool_results {
                    let tool_msg = crate::session::Message::new(
                        "tool".to_string(),
                        format!(
                            "Tool '{}' returned: {}",
                            result.tool_call_id, result.content
                        ),
                    );
                    session.add_message(&self.store, tool_msg).map_err(|e| {
                        LabeledError::new(format!("Failed to save tool message: {}", e))
                    })?;
                }
            }

            // Build conversation history from in-memory messages (NOT from session)
            // This ensures tool results are passed to LLM even without --session flag
            let history_prompt = {
                let history = conversation_messages
                    .iter()
                    .map(|(role, content)| format!("{}: {}", role, content))
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

            // Make another LLM call with conversation history
            ui_renderer.emit(&UiEvent::LlmStart);
            llm_response = call_llm_with_ui_ticks(
                &runtime,
                &self.runtime_ctx,
                &config,
                &history_prompt,
                tool_definitions.clone(),
                &mut ui_renderer,
            )
                .map_err(|e| {
                    LabeledError::new(format!("LLM call failed: {}", e.msg))
                        .with_label(e.msg, call.head)
                })?;
            ui_renderer.emit(&UiEvent::LlmEnd {
                response_chars: llm_response.text.len(),
                tool_calls: llm_response.tool_calls.len(),
            });

            // Track assistant response in conversation
            conversation_messages.push(("assistant".to_string(), llm_response.text.clone()));
        }

        // Capture tool call count before moving executed_tool_calls
        let tool_call_count = executed_tool_calls.len();

        // Build final response with all executed tool calls
        let final_response = crate::llm::LlmResponse {
            text: llm_response.text.clone(),
            usage: llm_response.usage.clone(),
            tool_calls: executed_tool_calls,
            tool_call_metadata: tool_results_metadata,
        };

        // Extract text for session storage
        let response_text = final_response.text.clone();

        // Save messages to session if active
        let mut message_count = 0;
        let mut compaction_count = 0;

        if let Some(mut session) = session_opt {
            // Create and save user message
            let user_msg = crate::session::Message::new("user".to_string(), prompt.clone());
            session
                .add_message(&self.store, user_msg)
                .map_err(|e| LabeledError::new(format!("Failed to save user message: {}", e)))?;

            // Create and save assistant message
            let assistant_msg =
                crate::session::Message::new("assistant".to_string(), response_text.clone());
            session
                .add_message(&self.store, assistant_msg)
                .map_err(|e| {
                    LabeledError::new(format!("Failed to save assistant message: {}", e))
                })?;

            // Check for compaction
            let _compacted = session.maybe_compact(&self.store).map_err(|e| {
                ui_renderer.emit(&UiEvent::Warning {
                    message: format!("Session compaction failed: {e}"),
                });
            });

            // Get session stats for metadata
            message_count = session.messages().len();
            compaction_count = session.compaction_count();
        }

        ui_renderer.emit(&UiEvent::Completed {
            tool_calls: tool_call_count,
        });
        ui_renderer.flush();

        // Format response with session metadata
        let response_value = crate::llm::format_response(
            &final_response,
            &config,
            final_session_id.as_deref(),
            compaction_count,
            call.head,
        );

        // Add message_count to _meta if session was used
        if final_session_id.is_some() {
            // Extract existing record
            if let Ok(record) = response_value.as_record() {
                let mut new_record = record.clone();

                // Update _meta field with message_count
                if let Some(meta_value) = new_record.get("_meta")
                    && let Ok(meta_record) = meta_value.as_record()
                {
                    let mut new_meta = meta_record.clone();
                    new_meta.insert(
                        "message_count".to_string(),
                        Value::int(message_count as i64, call.head),
                    );

                    new_record.insert("_meta".to_string(), Value::record(new_meta, call.head));

                    return Ok(Value::record(new_record, call.head));
                }
            }
        }

        Ok(response_value)
    }
}

/// Extract configuration from command-line flags.
///
/// Reads flags from the EvaluatedCall and returns a Config with values for
/// provided flags and None for unprovided flags.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing command flags
///
/// # Returns
/// Config with values from flags or Config::default() fields for unprovided flags
pub fn extract_flag_config(call: &EvaluatedCall) -> Config {
    // Helper to safely extract string flag
    fn get_string_flag(call: &EvaluatedCall, name: &str) -> Option<String> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_str().map(|s| s.to_string()).ok())
    }

    // Helper to safely extract float flag
    fn get_float_flag(call: &EvaluatedCall, name: &str) -> Option<f64> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_float().ok())
    }

    // Helper to safely extract u32 flag (from i64, rejecting negatives)
    fn get_u32_flag(call: &EvaluatedCall, name: &str) -> Option<u32> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_int().ok())
            .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    }

    // Extract all flags
    let provider = get_string_flag(call, "provider").unwrap_or_default();
    let model = get_string_flag(call, "model").unwrap_or_default();
    let api_key = get_string_flag(call, "api-key");
    let base_url = get_string_flag(call, "base-url");
    let temperature = get_float_flag(call, "temperature");
    let max_tokens = get_u32_flag(call, "max-tokens");
    let max_context_tokens = get_u32_flag(call, "max-context-tokens");
    let max_output_tokens = get_u32_flag(call, "max-output-tokens");
    let max_tool_turns = get_u32_flag(call, "max-turns");

    Config {
        provider,
        provider_impl: None,
        model,
        api_key,
        base_url,
        temperature,
        max_tokens,
        max_context_tokens,
        max_output_tokens,
        max_tool_turns,
    }
}

/// Resolve configuration from all sources with proper precedence.
///
/// NEW Resolution pipeline:
/// 1. Parse PluginConfig from $env.config.plugins.agent (if present)
/// 2. Determine active model:
///    - If --model flag provided: use it (provider/model format)
///    - Else if --small flag provided: use small_model from PluginConfig
///    - Else use config.model (default)
/// 3. Call PluginConfig::resolve_model() to get base Config
/// 4. Merge with flag overrides (temperature, max_tokens, etc.)
/// 5. Validate and return
///
/// FALLBACK for backward compatibility:
/// - If plugin config doesn't have new structure (no "providers" field)
/// - Fall back to OLD Config::from_plugin_config() behavior
/// - Support old --provider and --model flags (separate)
///
/// # Arguments
/// * `engine` - Engine interface for accessing plugin config
/// * `call` - The EvaluatedCall containing command flags
///
/// # Returns
/// Fully resolved and validated Config, or error if validation fails
pub fn resolve_config<E: EngineConfigInterface>(
    engine: &E,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Step 1: Get plugin config value (if present)
    let plugin_config_opt = engine.get_plugin_config()?;

    // Step 2: Try NEW plugin config structure first
    if let Some(ref plugin_value) = plugin_config_opt {
        // Try to parse as NEW PluginConfig structure
        if let Ok(plugin_config) = PluginConfig::from_plugin_config(plugin_value) {
            // NEW FLOW: Use PluginConfig
            return resolve_with_new_config(plugin_config, call);
        }
        // If parsing failed, fall through to OLD flow
    }

    // Step 3: FALLBACK to OLD flow for backward compatibility
    resolve_with_old_config(plugin_config_opt, call)
}

/// NEW resolution flow using PluginConfig structure
fn resolve_with_new_config(
    plugin_config: PluginConfig,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Helper to get string flag
    fn get_string_flag(call: &EvaluatedCall, name: &str) -> Option<String> {
        call.get_flag(name)
            .ok()
            .flatten()
            .and_then(|v: Value| v.as_str().map(|s| s.to_string()).ok())
    }

    // Helper to get bool flag (switch)
    fn get_bool_flag(call: &EvaluatedCall, name: &str) -> bool {
        call.get_flag(name).ok().flatten().unwrap_or(false)
    }

    // Determine which model to use (priority: --model > --small > config.model)
    let model_ref = if let Some(model_flag) = get_string_flag(call, "model") {
        // --model flag takes highest priority
        model_flag
    } else if get_bool_flag(call, "small") {
        // --small flag uses small_model from config
        plugin_config.small_model.clone().ok_or_else(|| {
            LabeledError::new("No small model configured").with_label(
                "Set 'small_model' in plugin config to use --small flag",
                call.head,
            )
        })?
    } else {
        // Use default model from config
        plugin_config.model.clone()
    };

    // Resolve model to Config using PluginConfig
    let mut config = plugin_config
        .resolve_model(&model_ref)
        .map_err(|msg| LabeledError::new("Failed to resolve model").with_label(msg, call.head))?;

    // Step 3: Apply flag overrides for optional fields
    // These override any values from PluginConfig
    if let Some(api_key) = get_string_flag(call, "api-key") {
        config.api_key = Some(api_key);
    }
    if let Some(base_url) = get_string_flag(call, "base-url") {
        config.base_url = Some(base_url);
    }
    if let Some(temperature) = call
        .get_flag::<Value>("temperature")
        .ok()
        .flatten()
        .and_then(|v| v.as_float().ok())
    {
        config.temperature = Some(temperature);
    }
    if let Some(max_tokens) = call
        .get_flag::<Value>("max-tokens")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_tokens = Some(max_tokens);
    }
    if let Some(max_context) = call
        .get_flag::<Value>("max-context-tokens")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_context_tokens = Some(max_context);
    }
    if let Some(max_output) = call
        .get_flag::<Value>("max-output-tokens")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_output_tokens = Some(max_output);
    }
    if let Some(max_turns) = call
        .get_flag::<Value>("max-turns")
        .ok()
        .flatten()
        .and_then(|v| v.as_int().ok())
        .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
    {
        config.max_tool_turns = Some(max_turns);
    }

    // Step 4: Validate final config
    config
        .validate()
        .map_err(|msg| LabeledError::new("Config validation failed").with_label(msg, call.head))?;

    Ok(config)
}

/// OLD resolution flow for backward compatibility
fn resolve_with_old_config(
    plugin_config_opt: Option<Value>,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    // Step 1: Extract flag config first
    let flag_config = extract_flag_config(call);

    // Step 2: Determine provider/model for env lookup
    // Use plugin config if available, then flags, then default
    let (provider_hint, model_hint) = if let Some(ref plugin_value) = plugin_config_opt {
        // Try to extract provider/model from plugin config for env lookup
        let plugin_parsed = Config::from_plugin_config(plugin_value)?;
        (plugin_parsed.provider.clone(), plugin_parsed.model.clone())
    } else if !flag_config.provider.is_empty() && !flag_config.model.is_empty() {
        (flag_config.provider.clone(), flag_config.model.clone())
    } else {
        ("openai".to_string(), "gpt-4".to_string())
    };

    // Step 3: Start with defaults and merge environment config
    let env_config = Config::from_env(&provider_hint, &model_hint);
    let mut config = Config::default().merge(env_config);

    // Step 4: Merge plugin config if present
    if let Some(plugin_value) = plugin_config_opt {
        let plugin_config = Config::from_plugin_config(&plugin_value)?;
        config = config.merge(plugin_config);
    }

    // Step 5: Merge flag config (highest precedence) - only if values are non-empty
    // For required fields, only override if non-empty
    if !flag_config.provider.is_empty() {
        config.provider = flag_config.provider;
    }
    if !flag_config.model.is_empty() {
        config.model = flag_config.model;
    }
    // For optional fields, use standard merge
    config.api_key = flag_config.api_key.or(config.api_key);
    config.base_url = flag_config.base_url.or(config.base_url);
    config.temperature = flag_config.temperature.or(config.temperature);
    config.max_tokens = flag_config.max_tokens.or(config.max_tokens);
    config.max_context_tokens = flag_config.max_context_tokens.or(config.max_context_tokens);
    config.max_output_tokens = flag_config.max_output_tokens.or(config.max_output_tokens);
    config.max_tool_turns = flag_config.max_tool_turns.or(config.max_tool_turns);

    // Step 6: Validate final config
    config
        .validate()
        .map_err(|msg| LabeledError::new("Config validation failed").with_label(msg, call.head))?;

    Ok(config)
}

pub mod session;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod prompt_tests;

#[cfg(test)]
mod tool_session_test;
