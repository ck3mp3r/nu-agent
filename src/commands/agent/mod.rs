use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, LabeledError, Signature, Type, Value};
use std::io::IsTerminal;
use std::time::Duration;

use crate::{
    AgentPlugin,
    config::{Config, PluginConfig},
    plugin::RuntimeCtx,
};

mod contracts;
mod conversation_runtime;
mod orchestrator;
mod session_resolver;
mod tool_handler;
mod ui;
mod ui_runtime;

use self::{
    conversation_runtime::AgentConversationRuntime,
    orchestrator::{run_hydrated_interactive_loop, run_interactive_loop, run_single_turn},
    session_resolver::{DefaultSessionResolver, SessionResolutionInput, SessionResolver},
    ui::policy::resolve_ui_policy,
    ui_runtime::{StderrProgressUi, TuiInteractiveUi},
};
use crate::commands::agent::ui::factory::UiRendererFactory;
use crate::commands::agent::ui::tui::platform::safety::RestoreRunError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentMode {
    Tui,
    Stderr,
}

impl AgentMode {
    fn is_tui(self) -> bool {
        matches!(self, Self::Tui)
    }
}

fn resolve_agent_mode(input_is_nothing: bool, stdin_is_tty: bool, stderr_is_tty: bool) -> AgentMode {
    if input_is_nothing && stdin_is_tty && stderr_is_tty {
        AgentMode::Tui
    } else {
        AgentMode::Stderr
    }
}

#[cfg(test)]
pub(crate) use session_resolver::{SessionRequest, generate_session_id, resolve_session_request};

#[cfg(test)]
pub(crate) fn materialize_pending_tui_session_if_needed(
    store: &crate::session::SessionStore,
    session_opt: &mut Option<crate::session::Session>,
    pending_tui_session_id: &mut Option<String>,
) -> Result<(), LabeledError> {
    if session_opt.is_some() {
        return Ok(());
    }

    let Some(session_id) = pending_tui_session_id.take() else {
        return Ok(());
    };

    let session = store
        .get_or_create(Some(session_id))
        .map_err(|e| LabeledError::new(format!("Failed to load/create session: {e}")))?;
    *session_opt = Some(session);
    Ok(())
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

pub(crate) fn builtin_fs_tool_definitions() -> Vec<rig::completion::ToolDefinition> {
    vec![
        rig::completion::ToolDefinition {
            name: "read".to_string(),
            description: "Read file content with optional line windowing and return content/version metadata".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 0 },
                    "limit": { "type": "integer", "minimum": 0 }
                },
                "required": ["path"]
            }),
        },
        rig::completion::ToolDefinition {
            name: "edit".to_string(),
            description: "Search/replace edit with compare-and-swap guard".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "search": { "type": "string" },
                    "replacement": { "type": "string" },
                    "expected_version": { "type": "string" },
                    "match_mode": { "type": "string", "enum": ["literal", "regex"] },
                    "occurrence": { "type": "string", "enum": ["first", "all"] }
                },
                "required": ["path", "search", "replacement", "expected_version"]
            }),
        },
        rig::completion::ToolDefinition {
            name: "patch".to_string(),
            description: "Apply line-range patch operations with compare-and-swap guard".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "expected_version": { "type": "string" },
                    "operations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "range": {
                                    "type": "object",
                                    "properties": {
                                        "start": { "type": "integer", "minimum": 1 },
                                        "end": { "type": "integer", "minimum": 1 }
                                    },
                                    "required": ["start", "end"]
                                },
                                "replacement": { "type": "string" }
                            },
                            "required": ["range", "replacement"]
                        }
                    }
                },
                "required": ["path", "expected_version", "operations"]
            }),
        },
    ]
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
                (Type::Nothing, Type::Nothing),
                (Type::Nothing, Type::Record(vec![].into())),
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
        let stdin_is_tty = std::io::stdin().is_terminal();
        let stderr_is_tty = std::io::stderr().is_terminal();
        let input_is_nothing = matches!(input, Value::Nothing { .. });
        let mode = resolve_agent_mode(input_is_nothing, stdin_is_tty, stderr_is_tty);

        let _foreground_guard = if mode.is_tui() {
            Some(engine.enter_foreground().map_err(|err| {
                LabeledError::new(format!(
                    "Failed to enter foreground for interactive TUI input: {err}"
                ))
            })?)
        } else {
            None
        };

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
                    LabeledError::new("Failed to resolve caller cwd").with_label(
                        format!("Unable to read current dir from Nushell engine: {e}"),
                        call.head,
                    )
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

        tool_definitions.extend(builtin_fs_tool_definitions());

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

        let resolver = DefaultSessionResolver::new(&self.store);
        let session_resolution = resolver.resolve(SessionResolutionInput {
            use_tui: mode.is_tui(),
            input_is_nothing,
            session_id,
            new_session,
        })?;

        // Create audit log directory ONCE before prompt loop
        let log_dir = crate::utils::xdg::data_dir()
            .map_err(|e| LabeledError::new(format!("XDG data directory error: {}", e)))?
            .join("nu-agent");
        std::fs::create_dir_all(&log_dir).map_err(|e| {
            LabeledError::new(format!("Failed to create audit log directory: {}", e))
        })?;
        let log_path = log_dir.join("tool_audit.log");

        let audit_logger = std::sync::Arc::new(crate::tools::audit::AuditLogger::new(log_path));
        let tool_executor = crate::tools::executor::ToolExecutor::new(
            std::sync::Arc::new(engine.clone()),
            audit_logger,
            tool_timeout,
        );

        let mut runtime_impl = AgentConversationRuntime {
            runtime,
            runtime_ctx: self.runtime_ctx.clone(),
            config,
            tool_definitions,
            closure_registry,
            mcp_registry,
            mcp_tool_server_handle,
            tool_executor,
            engine: engine.clone(),
            store: self.store.clone(),
            session: session_resolution.session,
            final_session_id: session_resolution.final_session_id,
        };

        match mode {
            AgentMode::Tui => run_tui_mode(
                &mut runtime_impl,
                input,
                input_is_nothing,
                call.head,
                ui_policy,
                session_resolution.tui_should_hydrate_transcript,
                session_resolution.tui_initial_messages,
            ),
            AgentMode::Stderr => run_stderr_mode(
                &mut runtime_impl,
                input,
                call.head,
                ui_policy,
                stderr_is_tty,
            ),
        }
    }
}

fn run_tui_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    input_is_nothing: bool,
    span: nu_protocol::Span,
    ui_policy: crate::commands::agent::ui::policy::UiPolicy,
    tui_should_hydrate_transcript: bool,
    tui_initial_messages: Vec<crate::commands::agent::contracts::UiMessageSnapshot>,
) -> Result<Value, LabeledError> {
    let mut terminal_lifecycle = ui::tui::platform::terminal::TerminalLifecycle::new(
        ui::tui::runtime::AnsiTerminalBackend::new(std::io::stderr()),
    );

    let (columns, rows) = crossterm::terminal::size().unwrap_or((120, 30));
    let fallback_events = ui::tui::runtime::open_tty_reader()
        .ok()
        .and_then(|tty_reader| {
            ui::tui::runtime::TtyTerminalEvents::new(tty_reader, Duration::from_millis(30)).ok()
        });

    let runtime_renderer = ui::tui::runtime::TuiRuntimeRenderer::new_live(
        ui::factory::StderrUiFactory::new(std::io::stderr(), false).create(ui_policy),
        ui::tui::runtime::HybridTerminalEvents::new(Duration::from_millis(60), fallback_events),
        columns,
        rows,
    )
    .map_err(|err| LabeledError::new(format!("Failed to initialize TUI renderer: {err}")))?;

    let mut tui_ui = TuiInteractiveUi::new(runtime_renderer);
    tui_ui.set_active_model_identity(format_active_model_identity(
        &runtime_impl.config.provider,
        &runtime_impl.config.model,
    ));

    let result = ui::tui::runtime::run_with_terminal_restore(&mut terminal_lifecycle, || {
        if input_is_nothing {
            if tui_should_hydrate_transcript {
                run_hydrated_interactive_loop(runtime_impl, &mut tui_ui, tui_initial_messages, span)
            } else {
                run_interactive_loop(runtime_impl, &mut tui_ui, span)
            }
        } else {
            let (prompt, context) = extract_prompt_and_context(input)?;
            run_single_turn(runtime_impl, &mut tui_ui, prompt, context, span)
        }
    });

    map_tui_run_result(result)
}

pub(crate) fn format_active_model_identity(provider: &str, model: &str) -> String {
    if model.starts_with(&format!("{provider}/")) {
        model.to_string()
    } else {
        format!("{provider}/{model}")
    }
}

fn run_stderr_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    span: nu_protocol::Span,
    ui_policy: crate::commands::agent::ui::policy::UiPolicy,
    stderr_is_tty: bool,
) -> Result<Value, LabeledError> {
    let mut stderr_ui = StderrProgressUi::new(
        ui::factory::StderrUiFactory::new(std::io::stderr(), stderr_is_tty).create(ui_policy),
    );
    let (prompt, context) = extract_prompt_and_context(input)?;
    run_single_turn(runtime_impl, &mut stderr_ui, prompt, context, span)
}

fn extract_prompt_and_context(input: &Value) -> Result<(String, Option<String>), LabeledError> {
    let prompt = extract_prompt_from_input(input)?;
    let context = extract_context_from_input(input)?;
    Ok((prompt, context))
}

fn map_tui_run_result(
    result: Result<Value, ui::tui::runtime::RuntimeRunError<LabeledError>>,
) -> Result<Value, LabeledError> {
    match result {
        Ok(value) => Ok(value),
        Err(ui::tui::runtime::RuntimeRunError::Enter(err)) => Err(LabeledError::new(format!(
            "Failed to enter TUI terminal lifecycle: {err}"
        ))),
        Err(ui::tui::runtime::RuntimeRunError::Run(RestoreRunError::Run(err))) => Err(err),
        Err(ui::tui::runtime::RuntimeRunError::Run(RestoreRunError::RunWithRestore {
            run_error,
            restore_error,
        })) => Err(LabeledError::new(format!(
            "TUI run failed and terminal restore failed: run={run_error}, restore={restore_error}"
        ))),
        Err(ui::tui::runtime::RuntimeRunError::Run(RestoreRunError::Restore(err))) => Err(
            LabeledError::new(format!("Failed to restore terminal after TUI run: {err}")),
        ),
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
mod orchestrator_test;

#[cfg(test)]
mod session_resolver_test;

#[cfg(test)]
mod tool_session_test;
