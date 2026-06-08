use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, Type, Value};
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
    session::CompactionStrategy,
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

fn is_builtin_enabled(name: &str, config: &crate::config::AgentsConfig) -> bool {
    use crate::agent::protocol::persona::builtins;
    match name {
        n if n == builtins::BUILTIN_PLANNER_NAME => config.planner_enabled,
        n if n == builtins::BUILTIN_MAKER_NAME => config.maker_enabled,
        _ => true, // non-builtins are always "enabled"
    }
}

fn resolve_default_agent(
    config: &crate::config::AgentsConfig,
) -> Result<Option<String>, LabeledError> {
    use crate::agent::protocol::persona::builtins;
    let default = &config.default;
    if builtins::is_builtin_persona(default) && !is_builtin_enabled(default, config) {
        // Default is disabled — try fallback
        match &config.fallback {
            Some(fallback) => Ok(Some(fallback.clone())),
            None => Err(LabeledError::new(format!(
                "Default agent '{}' is disabled and no fallback configured. \
                 Set `agents.fallback` in config or enable '{}'.",
                default, default
            ))),
        }
    } else {
        Ok(Some(default.clone()))
    }
}

fn resolve_effective_permissions_config(
    call: &EvaluatedCall,
    plugin_config: Option<&Value>,
    agent_overlay: Option<&PermissionsOverlay>,
) -> Result<(PermissionsConfig, String), LabeledError> {
    let base = PermissionsConfig::parse_from_plugin_config(plugin_config);
    let cli_permissions: Option<Value> = call.get_flag("permissions").ok().flatten();

    // Build permission chain: base → agent_overlay → CLI
    // CLI always wins (highest precedence)
    let mut effective = base;

    if let Some(overlay) = agent_overlay {
        effective = effective.with_overlay(overlay);
    }

    if let Some(value) = cli_permissions.as_ref() {
        let overlay = PermissionsOverlay::parse_from_cli_value(value).map_err(|msg| {
            LabeledError::new("Invalid --permissions value").with_label(msg, value.span())
        })?;
        effective = effective.with_overlay(&overlay);
    }

    let summary = effective.summary();
    let overlay_active = agent_overlay.is_some() || cli_permissions.is_some();
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
/// Returns the session_id as Option<String>.
/// Validates that flags are valid.
///
/// # Arguments
/// * `call` - The EvaluatedCall containing session flags
///
/// # Returns
/// An `Option<String>` representing the session ID.
///
/// # Errors
/// Returns an error if flags are invalid.
pub fn extract_and_validate_session_flags(
    call: &EvaluatedCall,
) -> Result<Option<String>, LabeledError> {
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

/// Extract tool name patterns from --tool-filter flag.
///
/// Expected input is a list of strings, e.g. ["k8s__*", "gh__list_*"]
///
/// Returns an empty vector when the flag is not provided.
/// Empty vector means "no filtering" (match all tools).
pub fn extract_tool_filter_from_call(call: &EvaluatedCall) -> Result<Vec<String>, LabeledError> {
    args::extract_tool_filter_from_call(call)
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

pub(crate) fn builtin_tool_definitions() -> Vec<rig::completion::ToolDefinition> {
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

pub(crate) fn orchestrator_tool_definitions(
    available_agents: &[crate::agent::protocol::persona::PersonaSummary],
) -> Vec<rig::completion::ToolDefinition> {
    let description = if available_agents.is_empty() {
        "Spawn a new agent in a tmux pane. No agent personas found. Create .agents/<name>.md files to define agents.".to_string()
    } else {
        let mut desc = String::from(
            "Spawn a new agent in a tmux pane (in a window called \"agents\"). \
             Communicate with spawned agents via `send_message`. \
             The user can also interact directly with spawned agent panes.\n\n\
             Available agents:\n",
        );
        for agent in available_agents {
            desc.push_str(&format!("- {}", agent.name));
            if let Some(ref d) = agent.description {
                desc.push_str(&format!(": {}", d));
            }
            desc.push('\n');
        }
        desc
    };

    vec![
        rig::completion::ToolDefinition {
            name: "spawn_agent".to_string(),
            description,
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent": { "type": "string", "description": "Persona name (loads .agents/<name>.md)" },
                    "name": { "type": "string", "description": "Instance identity (optional, defaults to agent-N)" }
                },
                "required": ["agent"]
            }),
        },
        rig::completion::ToolDefinition {
            name: "terminate_agent".to_string(),
            description:
                "Terminate a running sub-agent by name. Kills its tmux pane and deregisters it."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Agent name to terminate" }
                },
                "required": ["name"]
            }),
        },
    ]
}

pub(crate) fn messaging_tool_definitions() -> Vec<rig::completion::ToolDefinition> {
    vec![
        rig::completion::ToolDefinition {
            name: "send_message".to_string(),
            description: "Send a message to another agent. Messages are delivered as conversation turns to the target agent. \
                           The target agent name must match a spawned agent's --name (use list_agents to discover running agents). \
                           The response comes back asynchronously as a new conversation turn.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": { "type": "string", "description": "Target agent name" },
                    "message": { "type": "string", "description": "Message content" },
                    "kind": {
                        "type": "string",
                        "description": "Message type: 'message' (generic/informational, default), 'task' (task assignment), 'completion' (task results), 'question' (blocked, needs decision)",
                        "enum": ["message", "task", "completion", "question"],
                        "default": "message"
                    }
                },
                "required": ["to", "message"]
            }),
        },
        rig::completion::ToolDefinition {
            name: "list_agents".to_string(),
            description: "List all connected agents and their names".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
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

    fn extra_description(&self) -> &str {
        "Pipe a prompt string to agent, or run interactively (no input = TUI mode).

Provider/model selection via --model provider/model or plugin config.
Supported providers: github-copilot, openai, anthropic, ollama

Session persistence via --session allows resuming conversations across invocations.

Tool closures via --tools enable custom Nushell functions as LLM tools.

Tool filtering via --tool-filter restricts exposed tools (builtin, MCP, closure) to glob patterns.
Applies to ALL tool types. Omit to expose all tools.

Permissions overlay via --permissions controls authorization for tool calls.

Agent personas via --agent loads instructions from:
  - .agents/<name>.md (project-local)
  - $XDG_CONFIG_HOME/nu-agent/agents/<name>.md (global, usually ~/.config/nu-agent/agents/)
Use --name to set multi-agent identity (defaults to persona name).

Persona file front matter (optional YAML):
  name: <string>                # Agent identity (overridden by --name)
  description: <string>         # Persona summary
  model: <provider/model>       # Default model (overridden by --model)
  tool_filter: [<globs>]        # Tool filtering (overridden by --tool-filter)
  permissions: <record>         # Authorization overlay (overridden by --permissions)

CLI flags override front matter values. Front matter overrides plugin config.

Compaction strategies via --compaction-strategy:
  - sliding_summary (default): LLM summarizes old messages, keeps recent verbatim window.
  - sliding_window: drops old messages, keeps only the last N. No LLM call.
  - token_truncate: keeps newest messages within a token budget (chars/4 estimate). No LLM call.

Compaction flags:
  --compaction-strategy <string>   Primary strategy (sliding_summary, sliding_window, token_truncate)
  --compaction-threshold <int>     Message count threshold for auto-compaction (default: 100)
  --keep-recent <int>              Recent messages to keep during compaction (default: 10)
  --token-budget <int>             Token budget for token_truncate strategy
  --proactive-threshold-pct <num>  Proactive compaction threshold 0.0-1.0 (default: 0.80)"
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "ai",
            "llm",
            "chat",
            "copilot",
            "openai",
            "anthropic",
            "ollama",
            "prompt",
            "agent",
            "persona",
            "filter",
            "tools",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Send a prompt via pipe",
                example: r#""explain this error" | agent"#,
                result: None,
            },
            Example {
                description: "Interactive TUI mode",
                example: "agent",
                result: None,
            },
            Example {
                description: "Use a specific model",
                example: r#""summarize" | agent --model openai/gpt-4o"#,
                result: None,
            },
            Example {
                description: "Resume a named session",
                example: "agent --session my-project",
                result: None,
            },
            Example {
                description: "Pass custom tool closures",
                example: r#""list files" | agent --tools { list_files: {|| ls | get name} }"#,
                result: None,
            },
            Example {
                description: "Auto-approve all tool calls",
                example: r#""fix the tests" | agent --permissions { default: allow }"#,
                result: None,
            },
            Example {
                description: "Use an agent persona",
                example: r#""implement the feature" | agent --agent coder"#,
                result: None,
            },
            Example {
                description: "Use persona with model override",
                example: r#""research this topic" | agent --agent researcher --model anthropic/claude-sonnet-4-20250514"#,
                result: None,
            },
            Example {
                description: "Filter tools to specific patterns",
                example: r#""analyze code" | agent --tool-filter ['read' 'grep' 'glob']"#,
                result: None,
            },
            Example {
                description: "Use sliding_window compaction strategy",
                example: r#"agent --compaction-strategy sliding_window"#,
                result: None,
            },
            Example {
                description: "Custom compaction threshold and keep-recent count",
                example: r#"agent --compaction-threshold 50 --keep-recent 5"#,
                result: None,
            },
        ]
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
                "tool-filter",
                nu_protocol::SyntaxShape::List(Box::new(nu_protocol::SyntaxShape::String)),
                "List of tool name glob patterns, e.g. ['k8s__*', 'gh__list_*']",
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
                "verbose",
                "Increase UX detail; repeat for more detail (-v, -vv, -vvv)",
                Some('v'),
            )
            .switch(
                "quiet",
                "Suppress non-essential UX progress output",
                Some('q'),
            )
            .named(
                "log-level",
                nu_protocol::SyntaxShape::String,
                "Log level for file logging: off, error, warn, info, debug, trace (default: off)",
                None,
            )
            .named(
                "agent",
                nu_protocol::SyntaxShape::String,
                "Agent persona name (loads .agents/<name>.md)",
                None,
            )
            .named(
                "name",
                nu_protocol::SyntaxShape::String,
                "Agent instance identity for multi-agent messaging",
                None,
            )
            .named(
                "broker-socket",
                nu_protocol::SyntaxShape::String,
                "Broker socket path (internal)",
                None,
            )
            .named(
                "broker-token",
                nu_protocol::SyntaxShape::String,
                "Broker auth token (internal)",
                None,
            )
            .named(
                "parent-name",
                nu_protocol::SyntaxShape::String,
                "Parent agent name for sub-agent reporting (internal)",
                None,
            )
            .named(
                "compaction-strategy",
                nu_protocol::SyntaxShape::String,
                "Compaction strategy: sliding_summary, sliding_window, token_truncate",
                None,
            )
            .named(
                "compaction-threshold",
                nu_protocol::SyntaxShape::Int,
                "Message count threshold for auto-compaction",
                None,
            )
            .named(
                "keep-recent",
                nu_protocol::SyntaxShape::Int,
                "Number of recent messages to keep during compaction",
                None,
            )
            .named(
                "token-budget",
                nu_protocol::SyntaxShape::Int,
                "Token budget for token_truncate strategy",
                None,
            )
            .named(
                "proactive-threshold-pct",
                nu_protocol::SyntaxShape::Number,
                "Proactive compaction threshold (0.0-1.0)",
                None,
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

        // Initialize file logging if --log-level is provided
        if let Some(log_level_str) = call.get_flag::<String>("log-level")? {
            let log_level_str = log_level_str.to_lowercase();
            if log_level_str != "off" {
                // Parse log level
                let log_level = match log_level_str.as_str() {
                    "error" => log::LevelFilter::Error,
                    "warn" => log::LevelFilter::Warn,
                    "info" => log::LevelFilter::Info,
                    "debug" => log::LevelFilter::Debug,
                    "trace" => log::LevelFilter::Trace,
                    _ => {
                        return Err(LabeledError::new(format!(
                            "Invalid log level '{}'. Valid values: off, error, warn, info, debug, trace",
                            log_level_str
                        )));
                    }
                };

                // Resolve log directory: $XDG_STATE_HOME/nu-agent/logs
                let log_dir = crate::utils::xdg::state_dir()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to resolve XDG state directory: {e}"))
                    })?
                    .join("nu-agent")
                    .join("logs");

                // Create log directory
                std::fs::create_dir_all(&log_dir).map_err(|e| {
                    LabeledError::new(format!(
                        "Failed to create log directory {}: {}",
                        log_dir.display(),
                        e
                    ))
                })?;

                // Open log file in append mode
                let log_file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_dir.join("agent.log"))
                    .map_err(|e| LabeledError::new(format!("Failed to open log file: {e}")))?;

                // Initialize env_logger with file target
                let _ = env_logger::Builder::new()
                    .filter_level(log_level)
                    .target(env_logger::Target::Pipe(Box::new(log_file)))
                    .format_timestamp_millis()
                    .try_init(); // Ignore error if already initialized
            }
        }

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
        let session_id = extract_and_validate_session_flags(call)?;

        // Resolve configuration from all sources with proper precedence:
        // default < env < plugin < flags
        let mut config = resolve_config(engine, call)?;

        // Apply mode-specific defaults for max_tool_turns if not explicitly configured
        // User-specified value (via --max-turns or config file) always wins
        if config.max_tool_turns.is_none() && !mode.is_tui() {
            config.max_tool_turns = Some(20); // Pipeline mode gets 20, TUI stays unlimited (None)
        }

        // Extract tool timeout for ToolExecutor
        let tool_timeout = extract_tool_timeout(call);

        // Extract tools from --tools flag and build ClosureRegistry
        let tools_map = extract_tools_from_call(call)?;
        let mut closure_registry = crate::tools::closure::ClosureRegistry::new();
        for (name, closure) in tools_map {
            let params = crate::tools::closure::resolve_closure_params(&closure, engine);
            closure_registry.register(
                name,
                crate::tools::closure::ResolvedClosure { closure, params },
            );
        }

        // Extract optional tool name patterns.
        // Empty patterns means "no filtering" (match all tools).
        // This is extracted early because persona hasn't been loaded yet - we'll merge it later
        let cli_tool_filter_patterns = extract_tool_filter_from_call(call)?;

        let plugin_config_value = engine.get_plugin_config()?;

        let agents_config = plugin_config_value
            .as_ref()
            .and_then(|v| PluginConfig::from_plugin_config(v).ok())
            .map(|c| c.agents)
            .unwrap_or_default();

        let mcp_config = plugin_config_value
            .as_ref()
            .map(crate::tools::mcp::config::McpConfig::from_plugin_config)
            .transpose()
            .map_err(|err| {
                LabeledError::new("Failed to load MCP config")
                    .with_label(err.to_string(), call.head)
            })?;

        // Load agent persona and extract permissions overlay before resolving effective permissions
        let (agent_name, cli_name) = args::extract_agent_flags(call);
        log::debug!("agent flags: agent_name={agent_name:?}, cli_name={cli_name:?}");

        // Extract broker flags early (before runtime construction)
        let broker_flags = args::extract_broker_flags(call)?;
        log::debug!("broker flags: present={}", broker_flags.is_some());

        // Determine effective agent name:
        // 1. CLI --agent flag provided → validate it's not a disabled built-in
        // 2. No CLI flag → resolve from config default/fallback
        let effective_agent_name = if let Some(ref name) = agent_name {
            if crate::agent::protocol::persona::builtins::is_builtin_persona(name)
                && !is_builtin_enabled(name, &agents_config)
            {
                return Err(LabeledError::new(format!(
                    "Agent '{}' is disabled in config. Enable it or use a different agent.",
                    name
                ))
                .with_label("disabled agent", call.head));
            }
            Some(name.clone())
        } else {
            resolve_default_agent(&agents_config)?
        };
        log::debug!("effective_agent_name={effective_agent_name:?}");

        let persona = if let Some(ref name) = effective_agent_name {
            let cwd = engine.get_current_dir().map_err(|e| {
                LabeledError::new("Failed to get current directory")
                    .with_label(format!("{}", e), call.head)
            })?;
            let cwd = std::path::PathBuf::from(cwd);
            let config_dir = crate::utils::xdg::config_dir()
                .map(|base| base.join("nu-agent"))
                .map_err(|e| {
                    LabeledError::new("Cannot determine config directory")
                        .with_label(e.to_string(), call.head)
                })?;

            use crate::agent::protocol::persona::{
                FrontMatterParser, FsPersonaResolver, PersonaFileResolver,
                PulldownCmarkFrontMatterParser, interpret_front_matter,
            };

            let resolver = FsPersonaResolver::new(cwd, config_dir, agents_config.clone());
            let (_path, contents) = resolver.resolve(name).map_err(|e| {
                LabeledError::new("Agent persona not found").with_label(e.to_string(), call.head)
            })?;

            let parser = PulldownCmarkFrontMatterParser;
            let raw = parser.parse(&contents).map_err(|e| {
                LabeledError::new("Invalid agent persona front matter")
                    .with_label(e.to_string(), call.head)
            })?;

            // Interpret front matter into typed fields
            Some(
                interpret_front_matter(raw.front_matter.as_ref(), raw.body).map_err(|e| {
                    LabeledError::new("Invalid agent persona front matter")
                        .with_label(e.to_string(), call.head)
                })?,
            )
        } else {
            None
        };
        log::debug!(
            "persona loaded: name={:?}, model={:?}, tool_filter={:?}, has_permissions={}, body_len={}",
            persona.as_ref().and_then(|p| p.name.as_ref()),
            persona.as_ref().and_then(|p| p.model.as_ref()),
            persona.as_ref().and_then(|p| p.tool_filter.as_ref()),
            persona.as_ref().is_some_and(|p| p.permissions.is_some()),
            persona.as_ref().map_or(0, |p| p.body.len())
        );

        // Wire name field with precedence: CLI --name > front matter name > effective agent name
        let agent_identity = cli_name
            .or_else(|| persona.as_ref().and_then(|p| p.name.clone()))
            .or_else(|| effective_agent_name.clone());
        log::debug!("resolved agent_identity={agent_identity:?}");

        // Wire permissions field
        let agent_permissions_overlay = persona
            .as_ref()
            .and_then(|p| p.permissions.as_ref())
            .map(PermissionsOverlay::parse_from_yaml)
            .transpose()
            .map_err(|msg| {
                LabeledError::new("Invalid agent permissions").with_label(msg, call.head)
            })?;
        log::debug!(
            "agent_permissions_overlay present={}",
            agent_permissions_overlay.is_some()
        );

        // Wire tool_filter with precedence: CLI --tool-filter > front matter tool_filter
        let tool_filter_patterns = if cli_tool_filter_patterns.is_empty() {
            persona
                .as_ref()
                .and_then(|p| p.tool_filter.clone())
                .unwrap_or_default()
        } else {
            cli_tool_filter_patterns
        };
        log::debug!("effective tool_filter_patterns={tool_filter_patterns:?}");

        // Wire model with precedence: CLI --model > front matter model > plugin config
        // Config already has plugin/env/default merged, we just need to inject persona model if CLI didn't provide one
        let cli_model_provided = call.get_flag::<Value>("model").ok().flatten().is_some();
        runtime_build::apply_persona_model(
            &mut config,
            persona.as_ref().and_then(|p| p.model.as_deref()),
            cli_model_provided,
        );
        log::debug!(
            "effective model after persona merge: provider={}, model={}",
            config.provider,
            config.model
        );

        let (effective_permissions, permissions_startup_summary) =
            resolve_effective_permissions_config(
                call,
                plugin_config_value.as_ref(),
                agent_permissions_overlay.as_ref(),
            )?;

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
            select_mcp_tools(mcp_runtime.discovered_tools(), &tool_filter_patterns)
        } else {
            Vec::new()
        };

        let tool_server_handle = mcp_runtime
            .as_ref()
            .map(|r| r.tool_server_handle())
            .unwrap_or_else(|| rig::tool::server::ToolServer::new().run());

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
        let mut tool_definitions: Vec<rig::completion::ToolDefinition> = closure_registry
            .names()
            .map(|name| {
                let resolved = closure_registry.get(name).unwrap();
                crate::tools::closure::closure_to_tool_definition(
                    name.clone(),
                    &resolved.params,
                    None,
                )
            })
            .collect();

        tool_definitions.extend(builtin_tool_definitions());

        // Only add orchestrator tools (spawn_agent) for parent agents (no broker_flags)
        let is_orchestrator = broker_flags.is_none();

        // Add messaging tools when agent has broker access (child) or is orchestrator (parent)
        let has_messaging = broker_flags.is_some() || is_orchestrator;
        if has_messaging {
            tool_definitions.extend(messaging_tool_definitions());
        }
        let available_agents = if is_orchestrator {
            use crate::agent::protocol::persona::{FsPersonaResolver, PersonaLister};
            let cwd = engine
                .get_current_dir()
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            let config_dir = crate::utils::xdg::config_dir()
                .map(|base| base.join("nu-agent"))
                .unwrap_or_default();
            let resolver = FsPersonaResolver::new(cwd, config_dir, agents_config.clone());
            resolver.list_available()
        } else {
            Vec::new()
        };
        if is_orchestrator {
            tool_definitions.extend(orchestrator_tool_definitions(&available_agents));
        }

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

        // Apply tool filter patterns to ALL tools (builtin + closure + MCP) if provided
        // Store baseline (unfiltered) for agent switching
        let baseline_tool_definitions = tool_definitions.clone();
        if !tool_filter_patterns.is_empty() {
            log::debug!(
                "tool filter: patterns={tool_filter_patterns:?}, pre_filter_count={}",
                tool_definitions.len()
            );
            tool_definitions.retain(|td| {
                crate::tools::mcp::filter::matches_patterns(&td.name, &tool_filter_patterns)
            });
            log::debug!("tool filter: post_filter_count={}", tool_definitions.len());
        }

        let resolver = DefaultSessionResolver::new(&self.store);
        let mut session_resolution = resolver.resolve(SessionResolutionInput {
            use_tui: mode.is_tui(),
            input_is_nothing,
            session_id,
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

        // Register closure tools with the ToolServer
        // This happens once at startup, not per-turn
        use crate::agent::hook::closure_adapter::adapt_closures;

        let closure_tools = adapt_closures(
            &closure_registry,
            std::sync::Arc::new(tool_executor.clone()),
            call.head,
        );

        for tool in closure_tools {
            runtime
                .block_on(async { tool_server_handle.add_tool(tool).await })
                .map_err(|e| {
                    LabeledError::new(format!("Failed to register closure tool: {}", e))
                        .with_label(format!("{}", e), call.head)
                })?;
        }

        // Register builtin FS tools (read, edit, patch, skill) with ToolServer
        use crate::agent::hook::builtin_adapter::adapt_builtins;

        let cwd = engine.get_current_dir().map_err(|e| {
            LabeledError::new(format!("Failed to get current directory: {}", e))
                .with_label(format!("{}", e), call.head)
        })?;
        let cwd_path = std::path::PathBuf::from(cwd);

        let mut builtin_defs = builtin_tool_definitions();
        if has_messaging {
            builtin_defs.extend(messaging_tool_definitions());
        }

        // Create orchestrator state for parent agents (no broker_flags = orchestrator)
        // Also register the orchestrator itself in the AgentRegistry so child agents
        // can send messages back to it via send_message(to: "<orchestrator_name>").
        let (orchestrator_state, orchestrator_mailbox_rx) = if is_orchestrator {
            builtin_defs.extend(orchestrator_tool_definitions(&available_agents));
            let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::agent::mailbox::AgentRegistry::new(),
            ));

            // Register the orchestrator in its own registry
            let orchestrator_name = agent_identity
                .clone()
                .unwrap_or_else(|| "orchestrator".to_string());
            let (tokio_tx, mut tokio_rx) =
                tokio::sync::mpsc::channel::<crate::agent::mailbox::ServerFrame>(64);
            runtime.block_on(async {
                registry
                    .write()
                    .await
                    .add_connected(orchestrator_name.clone(), tokio_tx);
            });
            log::debug!(
                "Registered orchestrator '{}' in agent registry",
                orchestrator_name
            );

            // Bridge tokio channel to std::sync::mpsc for mailbox_rx
            let (std_tx, std_rx) =
                std::sync::mpsc::channel::<crate::agent::mailbox::IncomingMessage>();
            runtime.spawn(async move {
                while let Some(frame) = tokio_rx.recv().await {
                    if let crate::agent::mailbox::ServerFrame::Message {
                        from,
                        message,
                        kind,
                    } = frame
                    {
                        log::trace!("Orchestrator received message from '{}': {}", from, message);
                        if std_tx
                            .send(crate::agent::mailbox::IncomingMessage {
                                from,
                                message,
                                kind,
                            })
                            .is_err()
                        {
                            log::debug!(
                                "Orchestrator mailbox receiver dropped, stopping forwarding task"
                            );
                            break;
                        }
                    }
                }
            });

            let state = Some(std::sync::Arc::new(std::sync::Mutex::new(
                crate::agent::tools::handler::spawn_agent::OrchestratorState {
                    agent_identity: agent_identity.clone(),
                    ..crate::agent::tools::handler::spawn_agent::OrchestratorState::new(
                        registry,
                        cwd_path.clone(),
                    )
                },
            )));
            (state, Some(std_rx))
        } else {
            (None, None)
        };

        // Merge compaction config: default < plugin_config < CLI flags
        // Extract compaction config from plugin config (if available)
        let plugin_compaction = plugin_config_value
            .as_ref()
            .and_then(|v| PluginConfig::from_plugin_config(v).ok())
            .and_then(|pc| pc.compaction);

        // Extract compaction flags from CLI
        let cli_compaction = args::extract_compaction_flags(call)?;

        // Merge: plugin config overrides defaults, CLI overrides plugin config
        let merged_compaction =
            runtime_build::merge_compaction_configs(plugin_compaction.as_ref(), &cli_compaction);

        // Validate merged compaction config
        merged_compaction.validate().map_err(|msg| {
            LabeledError::new("Compaction config validation failed").with_label(msg, call.head)
        })?;

        // Build SessionConfig from merged compaction config
        let session_config = runtime_build::build_session_config(&merged_compaction);

        // Extract compaction policy fields (not in SessionConfig)
        let compaction_strategy = session_config.compaction_strategy;
        let compaction_proactive_threshold_pct =
            merged_compaction.proactive_threshold_pct.unwrap_or(0.80);
        let compaction_fallback_strategies = merged_compaction
            .fallback_strategies
            .unwrap_or_else(|| vec![CompactionStrategy::SlidingWindow]);

        // Apply config to session and extract session metadata
        let (compaction_threshold, compaction_count) =
            if let Some(ref mut session) = session_resolution.session {
                session.set_config(session_config);
                (
                    Some(session.config().compaction_threshold),
                    session.compaction_count(),
                )
            } else {
                (None, 0)
            };

        // Connect to broker if flags provided
        let (broker_sender, mailbox_rx, parent_name) = if let Some(flags) = broker_flags {
            log::debug!("Connecting to broker at {:?}", flags.socket_path);
            let client = runtime
                .block_on(async {
                    crate::agent::mailbox::BrokerClient::connect(&flags.socket_path, &flags.token)
                        .await
                })
                .map_err(|e| LabeledError::new(format!("Failed to connect to broker: {}", e)))?;

            log::debug!("Connected to broker as '{}'", client.name);

            let (sender, mut receiver) = client.split();

            // Spawn a task to forward broker messages to a channel
            let (tx, rx) = std::sync::mpsc::channel();
            runtime.spawn(async move {
                loop {
                    match receiver.recv().await {
                        Ok(crate::agent::mailbox::ServerFrame::Message {
                            from,
                            message,
                            kind,
                        }) => {
                            log::trace!("Received message from '{}': {}", from, message);
                            if tx
                                .send(crate::agent::mailbox::IncomingMessage {
                                    from,
                                    message,
                                    kind,
                                })
                                .is_err()
                            {
                                log::debug!("Mailbox receiver dropped, stopping forwarding task");
                                break;
                            }
                        }
                        Ok(_frame) => {
                            log::trace!("Ignoring non-message frame");
                        }
                        Err(e) => {
                            log::debug!("Broker receiver error: {}", e);
                            break;
                        }
                    }
                }
            });

            (Some(sender), Some(rx), flags.parent_name)
        } else {
            (None, orchestrator_mailbox_rx, None)
        };

        let broker_sender_arc =
            broker_sender.map(|s| std::sync::Arc::new(tokio::sync::Mutex::new(s)));
        let builtin_tools = adapt_builtins(
            builtin_defs,
            cwd_path,
            orchestrator_state.clone(),
            broker_sender_arc.clone(),
            agent_identity.clone(),
        );

        for tool in builtin_tools {
            runtime
                .block_on(async { tool_server_handle.add_tool(tool).await })
                .map_err(|e| {
                    LabeledError::new(format!("Failed to register builtin tool: {}", e))
                        .with_label(format!("{}", e), call.head)
                })?;
        }

        let mcp_caller_cwd: Option<std::path::PathBuf> =
            engine.get_current_dir().ok().map(std::path::PathBuf::from);

        // --- Cache preamble components (loaded once, reused every turn) ---
        let loaded_agents_result = mcp_caller_cwd
            .as_deref()
            .map(crate::agent::protocol::agents::load_agents_chain_for_cwd)
            .unwrap_or_default();

        for warning in &loaded_agents_result.warnings {
            log::warn!("AGENTS.md load warning: {}", warning);
        }

        let cached_agents_chain = loaded_agents_result.merged_chain;

        let cached_available_skills = mcp_caller_cwd
            .as_deref()
            .and_then(crate::agent::protocol::skills::render_available_skills_preamble);

        let cached_sub_agent_instruction = parent_name.as_ref().map(|parent| {
            format!(
                "You are a sub-agent. When you have completed your task, report your results back \
                 to your parent agent using the send_message tool with kind 'completion': \
                 send_message(to: \"{parent}\", message: \"<your results>\", kind: \"completion\"). \
                 If you are blocked and need a decision from your parent, use kind 'question': \
                 send_message(to: \"{parent}\", message: \"<your question>\", kind: \"question\"). \
                 Work autonomously — only use 'question' when truly blocked."
            )
        });

        let mut runtime_impl = AgentConversationRuntime {
            runtime,
            runtime_ctx: self.runtime_ctx.clone(),
            config,
            tool_definitions,
            baseline_tool_definitions,
            closure_registry,
            mcp_registry,
            mcp_runtime,
            mcp_tool_server_handle: tool_server_handle,
            mcp_lifecycle_projection,
            mcp_server_configs: mcp_config
                .as_ref()
                .map(|cfg| cfg.mcp.clone())
                .unwrap_or_default(),
            tool_filter_patterns,
            mcp_caller_cwd,
            tool_executor,
            engine: engine.clone(),
            store: self.store.clone(),
            final_session_id: session_resolution.final_session_id,
            compaction_threshold,
            compaction_count,
            auto_compaction_tolerance: 0,
            auto_compaction_hysteresis_margin: 0,
            auto_compaction_state: CompactionTriggerState::default(),
            compaction_strategy,
            compaction_proactive_threshold_pct,
            compaction_fallback_strategies,
            startup_plugin_config: plugin_config_value
                .as_ref()
                .and_then(|value| PluginConfig::from_plugin_config(value).ok()),
            permissions: effective_permissions,
            permissions_startup_summary,
            permissions_startup_emitted: false,
            session_grants: SessionGrantCache::default(),
            ask_hook: AsyncAskHook::new(AskRuntimeConfig {
                interactive: mode.is_tui(),
                non_interactive_mode: resolve_non_interactive_ask_mode(
                    plugin_config_value.as_ref(),
                )?,
                ..AskRuntimeConfig::default()
            }),
            memory: rig::memory::InMemoryConversationMemory::new(),
            conversation_store: crate::session::JsonlConversationStore::new(
                self.store.cache_dir().to_path_buf(),
            ),
            memory_message_count: 0,
            memory_hydrated: false,
            cached_client: None,
            cached_client_key: None,
            agent_persona_body: persona.as_ref().map(|p| p.body.clone()),
            agent_identity,
            agent_description: persona.as_ref().and_then(|p| p.description.clone()),
            orchestrator: orchestrator_state,
            broker_sender: broker_sender_arc,
            mailbox_rx,
            parent_name,
            compacting: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            available_agent_summaries: available_agents,
            agents_config,
            cached_agents_chain,
            cached_available_skills,
            cached_sub_agent_instruction,
        };
        log::debug!(
            "runtime: agent_persona_body_len={:?}, agent_identity={:?}, agent_description={:?}",
            runtime_impl.agent_persona_body.as_ref().map(|b| b.len()),
            runtime_impl.agent_identity,
            runtime_impl.agent_description
        );
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

pub(crate) fn build_agent_picker_catalog(
    available_agents: &[crate::agent::protocol::persona::PersonaSummary],
    active_agent: Option<&str>,
) -> Vec<crate::agent::ui::tui::state::AgentPickerOption> {
    use crate::agent::ui::tui::state::AgentPickerOption;
    available_agents
        .iter()
        .map(|agent| {
            let active = active_agent.is_some_and(|a| a == agent.name);
            let display = if agent.builtin {
                match &agent.description {
                    Some(desc) => format!("{} — {} [built-in]", agent.name, desc),
                    None => format!("{} [built-in]", agent.name),
                }
            } else {
                match &agent.description {
                    Some(desc) => format!("{} — {}", agent.name, desc),
                    None => agent.name.clone(),
                }
            };
            AgentPickerOption {
                name: agent.name.clone(),
                description: agent.description.clone(),
                display,
                active,
                builtin: agent.builtin,
            }
        })
        .collect()
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

#[cfg(test)]
mod args_test;

#[cfg(test)]
mod runtime_build_test;
