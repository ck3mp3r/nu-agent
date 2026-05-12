use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, LabeledError, Signature, Type, Value};
use std::io::IsTerminal;

mod args;
mod mode_execute;
mod runtime_build;

use crate::{
    AgentPlugin,
    agent::{
        conversation::runtime::AgentConversationRuntime,
        protocol::{compaction::CompactionTriggerState, contracts::UiMessageSnapshot},
        session::resolver::{DefaultSessionResolver, SessionResolutionInput, SessionResolver},
        tools::{
            authz::{
                AskRuntimeConfig, AsyncAskHook, NonInteractiveAskMode, PermissionsConfig,
                PermissionsOverlay, SessionGrantCache,
            },
            handler::McpToolRegistry,
        },
        ui::{
            policy::{UiPolicy, resolve_ui_policy},
            tui::{platform::safety::RestoreRunError, runtime::RuntimeRunError},
        },
    },
    config::{Config, PluginConfig},
    plugin::RuntimeCtx,
};

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

fn resolve_agent_mode(
    input_is_nothing: bool,
    stdin_is_tty: bool,
    stderr_is_tty: bool,
) -> AgentMode {
    if input_is_nothing && stdin_is_tty && stderr_is_tty {
        AgentMode::Tui
    } else {
        AgentMode::Stderr
    }
}

fn resolve_non_interactive_ask_mode(
    plugin_config: Option<&Value>,
) -> Result<NonInteractiveAskMode, LabeledError> {
    let Some(config) = plugin_config else {
        return Ok(NonInteractiveAskMode::Deny);
    };
    let Ok(record) = config.as_record() else {
        return Ok(NonInteractiveAskMode::Deny);
    };
    let Some(value) = record.get("non_interactive_ask") else {
        return Ok(NonInteractiveAskMode::Deny);
    };
    let raw = value.as_str().map_err(|_| {
        LabeledError::new("Invalid non_interactive_ask type").with_label(
            "non_interactive_ask must be 'deny' or 'allow'",
            config.span(),
        )
    })?;
    match raw {
        "deny" => Ok(NonInteractiveAskMode::Deny),
        "allow" => Ok(NonInteractiveAskMode::Allow),
        other => Err(
            LabeledError::new("Invalid non_interactive_ask value").with_label(
                format!("unsupported value '{other}'; expected 'deny' or 'allow'"),
                config.span(),
            ),
        ),
    }
}

fn resolve_effective_permissions_config(
    call: &EvaluatedCall,
    plugin_config: Option<&Value>,
) -> Result<(PermissionsConfig, String), LabeledError> {
    let base = PermissionsConfig::parse_from_plugin_config(plugin_config);
    let cli_permissions: Option<Value> = call.get_flag("permissions").ok().flatten();

    let effective = if let Some(value) = cli_permissions.as_ref() {
        let overlay = PermissionsOverlay::parse_from_cli_value(value).map_err(|msg| {
            LabeledError::new("Invalid --permissions value").with_label(msg, value.span())
        })?;
        base.with_overlay(&overlay)
    } else {
        base
    };

    let summary = effective.summary();
    let overlay_active = cli_permissions.is_some();
    let startup_message = format!(
        "permissions policy: overlay_active={} global={} tool_rules={} nu__run.command_rules={}",
        overlay_active,
        summary.global.as_str(),
        summary.tool_rule_count,
        summary.nu_run_command_rule_count,
    );

    Ok((effective, startup_message))
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
    crate::agent::protocol::prompt::merge_prompt_with_context(prompt, context)
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
    args::extract_and_validate_session_flags(call)
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
    args::extract_tools_from_call(call)
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
    args::extract_tool_timeout(call)
}

/// Extract MCP tool name patterns from --mcp-tools flag.
///
/// Expected input is a list of strings, e.g. ["k8s__*", "gh__list_*"]
///
/// Returns an empty vector when the flag is not provided.
/// Empty vector means "no filtering" (match all MCP tools).
pub fn extract_mcp_patterns_from_call(call: &EvaluatedCall) -> Result<Vec<String>, LabeledError> {
    args::extract_mcp_patterns_from_call(call)
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
            description: "Canonical edit contract with explicit mode (preview/apply), CAS guard, and legacy compatibility".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "mode": { "type": "string", "enum": ["preview", "apply"], "default": "apply" },
                    "expected_version": { "type": "string" },
                    "operation": {
                        "type": "object",
                        "properties": {
                            "type": { "type": "string", "enum": ["search_replace"], "default": "search_replace" },
                            "search": { "type": "string" },
                            "replacement": { "type": "string" },
                            "match_mode": { "type": "string", "enum": ["literal", "regex"], "default": "literal" },
                            "occurrence": { "type": "string", "enum": ["first", "all"], "default": "first" }
                        },
                        "required": ["search", "replacement"]
                    },
                    "search": { "type": "string", "description": "legacy compatibility field; prefer operation.search" },
                    "replacement": { "type": "string", "description": "legacy compatibility field; prefer operation.replacement" },
                    "match_mode": { "type": "string", "enum": ["literal", "regex"], "description": "legacy compatibility field; prefer operation.match_mode" },
                    "occurrence": { "type": "string", "enum": ["first", "all"], "description": "legacy compatibility field; prefer operation.occurrence" }
                },
                "required": ["path", "expected_version"]
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
        rig::completion::ToolDefinition {
            name: "skill".to_string(),
            description: "Load skill content by explicit name from local or home skill roots".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                },
                "required": ["name"]
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
                "permissions",
                nu_protocol::SyntaxShape::Record(vec![]),
                "Structured permissions overlay record for this run",
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

        let plugin_config_value = engine.get_plugin_config()?;

        let mcp_config = plugin_config_value
            .as_ref()
            .map(crate::tools::mcp::config::McpConfig::from_plugin_config)
            .transpose()
            .map_err(|err| {
                LabeledError::new("Failed to load MCP config")
                    .with_label(err.to_string(), call.head)
            })?;

        let (effective_permissions, permissions_startup_summary) =
            resolve_effective_permissions_config(call, plugin_config_value.as_ref())?;

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

        let mcp_lifecycle_projection =
            if let (Some(runtime), Some(cfg)) = (mcp_runtime.as_ref(), mcp_config.as_ref()) {
                runtime.lifecycle_projection(&cfg.mcp)
            } else {
                Vec::new()
            };

        let mcp_registry =
            McpToolRegistry::from_tools(discovered_mcp_tools.clone()).map_err(|msg| {
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
            mcp_lifecycle_projection,
            mcp_server_configs: mcp_config
                .as_ref()
                .map(|cfg| cfg.mcp.clone())
                .unwrap_or_default(),
            mcp_caller_cwd: engine.get_current_dir().ok().map(std::path::PathBuf::from),
            tool_executor,
            engine: engine.clone(),
            store: self.store.clone(),
            session: session_resolution.session,
            final_session_id: session_resolution.final_session_id,
            auto_compaction_tolerance: 0,
            auto_compaction_hysteresis_margin: 0,
            auto_compaction_state: CompactionTriggerState::default(),
            startup_plugin_config: plugin_config_value
                .as_ref()
                .and_then(|value| PluginConfig::from_plugin_config(value).ok()),
            permissions: effective_permissions,
            permissions_startup_summary,
            permissions_startup_emitted: false,
            session_grants: SessionGrantCache::default(),
            ask_hook: AsyncAskHook::new(AskRuntimeConfig {
                interactive: mode.is_tui(),
                non_interactive_mode:
                    resolve_non_interactive_ask_mode(plugin_config_value.as_ref())?,
                ..AskRuntimeConfig::default()
            }),
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
    ui_policy: UiPolicy,
    tui_should_hydrate_transcript: bool,
    tui_initial_messages: Vec<UiMessageSnapshot>,
) -> Result<Value, LabeledError> {
    mode_execute::run_tui_mode(
        runtime_impl,
        input,
        input_is_nothing,
        span,
        ui_policy,
        tui_should_hydrate_transcript,
        tui_initial_messages,
    )
}

pub(crate) fn format_active_model_identity(provider: &str, model: &str) -> String {
    if model.starts_with(&format!("{provider}/")) {
        model.to_string()
    } else {
        format!("{provider}/{model}")
    }
}

pub(crate) fn build_model_picker_catalog_from_plugin_config(
    plugin_config: &PluginConfig,
    active_model_identity: &str,
) -> Vec<crate::agent::ui::tui::state::ModelPickerOption> {
    let mut options = plugin_config
        .providers
        .iter()
        .flat_map(|(provider, provider_config)| {
            provider_config.models.keys().map(move |model| {
                let identity = format!("{provider}/{model}");
                crate::agent::ui::tui::state::ModelPickerOption {
                    provider: provider.clone(),
                    model: model.clone(),
                    identity: identity.clone(),
                    display: format!("{provider} / {model}"),
                    active: identity == active_model_identity,
                }
            })
        })
        .collect::<Vec<_>>();

    options.sort_by(|left, right| {
        left.provider
            .to_ascii_lowercase()
            .cmp(&right.provider.to_ascii_lowercase())
            .then_with(|| {
                left.model
                    .to_ascii_lowercase()
                    .cmp(&right.model.to_ascii_lowercase())
            })
    });
    options
}

pub(crate) fn model_picker_catalog_from_cached_startup_plugin_config(
    startup_plugin_config: Option<&PluginConfig>,
    active_model_identity: &str,
) -> Vec<crate::agent::ui::tui::state::ModelPickerOption> {
    startup_plugin_config
        .map(|config| build_model_picker_catalog_from_plugin_config(config, active_model_identity))
        .unwrap_or_default()
}

fn run_stderr_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    stderr_is_tty: bool,
) -> Result<Value, LabeledError> {
    mode_execute::run_stderr_mode(runtime_impl, input, span, ui_policy, stderr_is_tty)
}

fn extract_prompt_and_context(input: &Value) -> Result<(String, Option<String>), LabeledError> {
    let prompt = extract_prompt_from_input(input)?;
    let context = extract_context_from_input(input)?;
    Ok((prompt, context))
}

fn map_tui_run_result(
    result: Result<Value, RuntimeRunError<LabeledError>>,
) -> Result<Value, LabeledError> {
    match result {
        Ok(value) => Ok(value),
        Err(RuntimeRunError::Enter(err)) => Err(LabeledError::new(format!(
            "Failed to enter TUI terminal lifecycle: {err}"
        ))),
        Err(RuntimeRunError::Run(RestoreRunError::Run(err))) => Err(err),
        Err(RuntimeRunError::Run(RestoreRunError::RunWithRestore {
            run_error,
            restore_error,
        })) => Err(LabeledError::new(format!(
            "TUI run failed and terminal restore failed: run={run_error}, restore={restore_error}"
        ))),
        Err(RuntimeRunError::Run(RestoreRunError::Restore(err))) => Err(LabeledError::new(
            format!("Failed to restore terminal after TUI run: {err}"),
        )),
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
    runtime_build::extract_flag_config(call)
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
/// 4. Merge with flag overrides (temperature, max-context/output-tokens, etc.)
/// 5. Validate and return
///
/// FALLBACK for backward compatibility:
/// - If plugin config doesn't have new structure (no "providers" field)
/// - Fall back to OLD Config::from_plugin_config() behavior
/// - Model override remains authoritative via --model (provider/model format)
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
    runtime_build::resolve_with_new_config(plugin_config, call)
}

/// OLD resolution flow for backward compatibility
fn resolve_with_old_config(
    plugin_config_opt: Option<Value>,
    call: &EvaluatedCall,
) -> Result<Config, LabeledError> {
    runtime_build::resolve_with_old_config(plugin_config_opt, call)
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod prompt_test;
