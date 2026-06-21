use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, Type, Value};
use std::io::IsTerminal;

mod args;
pub(crate) mod input;
mod mode_execute;
mod permissions;
mod persona;
pub(crate) mod picker;
mod resolve_policy;
mod runtime_build;
mod setup;
pub(crate) mod tool_defs;

use permissions::resolve_effective_permissions_config;
use resolve_policy::resolve_ui_policy;
use tool_defs::{ToolAssembly, assemble_tool_definitions};

use crate::plugin::AgentPlugin;
use nu_agent_core::{
    config::{Config, PluginConfig},
    conversation::runtime::AgentConversationRuntime,
    policy::UiPolicy,
    session::resolver::{DefaultSessionResolver, SessionResolutionInput, SessionResolver},
    tools::{handler::McpToolRegistry},
};
use nu_agent_tui::RuntimeRunError;
use nu_agent_tui::platform::safety::RestoreRunError;

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

/// Whether the plugin should call `enter_foreground()` to receive SIGINT.
/// True for TUI (always needs it) and for stderr mode when stderr is a TTY
/// (user has a terminal and may press Ctrl+C).
fn should_enter_foreground(mode: AgentMode, stderr_is_tty: bool) -> bool {
    mode.is_tui() || stderr_is_tty
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

pub struct Agent {
    store: nu_agent_core::session::SessionStore,
}

impl Agent {
    /// Creates a new Agent command with the given SessionStore.
    pub fn new(store: nu_agent_core::session::SessionStore) -> Self {
        Self { store }
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

Permissions overlay via --permissions controls authorization for tool calls.

Agent personas via --agent loads instructions from:
  - .agents/<name>.md (project-local)
  - $XDG_CONFIG_HOME/nu-agent/agents/<name>.md (global, usually ~/.config/nu-agent/agents/)
Use --name to set multi-agent identity (defaults to persona name).

Persona file front matter (optional YAML):
  name: <string>                # Agent identity (overridden by --name)
  description: <string>         # Persona summary
  model: <provider/model>       # Default model (overridden by --model)
  permissions: <record>         # Authorization overlay (overridden by --permissions)

CLI flags override front matter values. Front matter overrides plugin config.

Compaction strategies via --compaction-strategy:
  - sliding_summary (default): LLM summarizes old messages, keeps recent verbatim window.
  - sliding_window: drops old messages, keeps only the last N. No LLM call.
  - token_truncate: keeps newest messages within a token budget (chars/4 estimate). No LLM call.

Compaction flags:
  --compaction-strategy <string>   Primary strategy (sliding_summary, sliding_window, token_truncate)
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
                description: "Use sliding_window compaction strategy",
                example: r#"agent --compaction-strategy sliding_window"#,
                result: None,
            },
            Example {
                description: "Custom keep-recent count",
                example: r#"agent --keep-recent 5"#,
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

        let cwd = std::path::PathBuf::from(engine.get_current_dir().map_err(|e| {
            LabeledError::new("Failed to resolve working directory")
                .with_label(format!("{e}"), call.head)
        })?);

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
                let log_dir = nu_agent_core::utils::xdg::state_dir()
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

        let _foreground_guard = if should_enter_foreground(mode, stderr_is_tty) {
            if mode.is_tui() {
                // TUI requires foreground — fail hard if it can't be obtained.
                Some(engine.enter_foreground().map_err(|err| {
                    LabeledError::new(format!(
                        "Failed to enter foreground for interactive TUI input: {err}"
                    ))
                })?)
            } else {
                // Stderr mode — best-effort so Ctrl+C works. Not fatal if it fails
                // (e.g. fully piped environment with no controlling terminal).
                engine.enter_foreground().ok()
            }
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
        let mut closure_registry = nu_agent_core::tools::closure::ClosureRegistry::new();
        for (name, closure) in tools_map {
            let params = nu_agent_core::tools::closure::resolve_closure_params(&closure, engine);
            closure_registry.register(
                name,
                nu_agent_core::tools::closure::ResolvedClosure { closure, params },
            );
        }

        let plugin_config_value = engine.get_plugin_config()?;

        let agents_config = plugin_config_value
            .as_ref()
            .and_then(|v| PluginConfig::from_plugin_config(v).ok())
            .map(|c| c.agents)
            .unwrap_or_default();

        let mcp_config = plugin_config_value
            .as_ref()
            .map(nu_agent_core::tools::mcp::config::McpConfig::from_plugin_config)
            .transpose()
            .map_err(|err| {
                LabeledError::new("Failed to load MCP config")
                    .with_label(err.to_string(), call.head)
            })?;

        // Load agent persona and resolve identity
        let (agent_name, cli_name) = args::extract_agent_flags(call);
        log::debug!("agent flags: agent_name={agent_name:?}, cli_name={cli_name:?}");
        let broker_flags = args::extract_broker_flags(call)?;
        log::debug!("broker flags: present={}", broker_flags.is_some());

        let call_has_model_flag = call.get_flag::<Value>("model").ok().flatten().is_some();
        let persona_resolution = persona::resolve_persona(
            agent_name,
            cli_name,
            &agents_config,
            &cwd,
            call,
            &mut config,
            call_has_model_flag,
        )?;
        let persona = persona_resolution.persona;
        let agent_identity = persona_resolution.agent_identity;
        let messaging_identity = persona_resolution.messaging_identity;
        let agent_permissions_overlay = persona_resolution.agent_permissions_overlay;

        let (effective_permissions, permissions_startup_summary) =
            resolve_effective_permissions_config(
                call,
                plugin_config_value.as_ref(),
                agent_permissions_overlay.as_ref(),
                mode.is_tui(),
            )?;

        // Create async runtime for LLM and MCP tool execution
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| LabeledError::new(format!("Failed to create async runtime: {}", e)))?;

        let mcp_runtime = if let Some(cfg) = mcp_config.as_ref() {
            if cfg.mcp.is_empty() {
                None
            } else {
                let caller_cwd_path = cwd.as_path();

                Some(
                    runtime
                        .block_on(nu_agent_core::tools::mcp::runtime::connect_servers(
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
            mcp_runtime.discovered_tools().to_vec()
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

        let ToolAssembly {
            tool_definitions,
            baseline_tool_definitions,
            available_agents,
            is_orchestrator,
            has_messaging,
        } = assemble_tool_definitions(
            &closure_registry,
            broker_flags.is_some(),
            &agents_config,
            &discovered_mcp_tools,
            &cwd,
        );

        let resolver = DefaultSessionResolver::new(&self.store);
        let mut session_resolution = resolver.resolve(SessionResolutionInput {
            use_tui: mode.is_tui(),
            input_is_nothing,
            session_id,
        })?;

        let setup::SetupResult {
            mailbox_rx,
            parent_name,
            merged_compaction,
            compaction_strategy,
            compaction_count,
        } = setup::register_tools(setup::RegisterToolsInput {
            runtime: &runtime,
            tool_server_handle: &tool_server_handle,
            closure_registry: &closure_registry,
            cwd: &cwd,
            engine,
            call,
            plugin_config_value: plugin_config_value.as_ref(),
            available_agents: &available_agents,
            messaging_identity: messaging_identity.clone(),
            broker_flags,
            is_orchestrator,
            has_messaging,
            tool_timeout,
            session: session_resolution.session.as_mut(),
        })?;

        let mcp_caller_cwd = cwd.clone();

        // ── Phase 8: Preamble cache ───────────────────────────────────────────
        let (cached_agents_chain, cached_available_skills, cached_sub_agent_instruction) =
            runtime_build::build_preamble_cache(&cwd, parent_name.as_deref());

        // ── Phase 9: Runtime construction ────────────────────────────────────
        let context_window_max_tokens = u64::from(config.resolved_max_context_tokens());
        let mut runtime_impl = runtime_build::build_runtime(runtime_build::RuntimeBuildParams {
            runtime,
            config,
            plugin_config_value,
            tool_definitions,
            baseline_tool_definitions,
            closure_registry,
            mcp_runtime,
            tool_server_handle,
            mcp_lifecycle_projection,
            mcp_server_configs: mcp_config
                .as_ref()
                .map(|cfg| cfg.mcp.clone())
                .unwrap_or_default(),
            mcp_caller_cwd: Some(mcp_caller_cwd),
            mcp_registry,
            engine: engine.clone(),
            store: self.store.clone(),
            final_session_id: session_resolution.final_session_id,
            context_window_max_tokens,
            compaction_threshold_pct: merged_compaction.proactive_threshold_pct.unwrap_or(0.80),
            compaction_count,
            compaction_strategy,
            effective_permissions,
            permissions_startup_summary,
            persona_body: persona.as_ref().map(|p| p.body.clone()),
            agent_identity,
            agent_description: persona.as_ref().and_then(|p| p.description.clone()),
            cached_agents_chain,
            cached_available_skills,
            cached_sub_agent_instruction,
            mailbox_rx,
            available_agents,
            agents_config,
        });
        log::debug!(
            "runtime: agent_persona_body_len={:?}, agent_identity={:?}, agent_description={:?}",
            runtime_impl.persona_state.persona_body_len(),
            runtime_impl.persona_state.agent_identity(),
            runtime_impl.persona_state.agent_description()
        );
        match mode {
            AgentMode::Tui => run_tui_mode(
                &mut runtime_impl,
                input,
                input_is_nothing,
                call.head,
                ui_policy,
                mode_execute::TuiHydrationInput {
                    should_hydrate: session_resolution.tui_should_hydrate_transcript,
                    initial_messages: session_resolution.tui_initial_messages,
                    last_total_tokens: session_resolution.last_total_tokens,
                },
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
    hydration: mode_execute::TuiHydrationInput,
) -> Result<Value, LabeledError> {
    mode_execute::run_tui_mode(
        runtime_impl,
        input,
        input_is_nothing,
        span,
        ui_policy,
        hydration,
    )
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
mod test_helpers;

#[cfg(test)]
mod input_test;

#[cfg(test)]
mod args_test;

#[cfg(test)]
mod permissions_test;

#[cfg(test)]
mod runtime_build_test;

#[cfg(test)]
mod tool_defs_test;

#[cfg(test)]
mod picker_test;

#[cfg(test)]
mod docs_contract_test;

#[cfg(test)]
mod resolve_policy_test;
