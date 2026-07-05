use nu_plugin::{EngineInterface, EvaluatedCall, PluginCommand, SimplePluginCommand};
use nu_protocol::{Category, Example, LabeledError, Signature, Type, Value};

mod args;
pub(crate) mod input;
mod mode_execute;
mod permissions;
mod persona;
pub(crate) mod picker;
mod resolve_policy;
mod run_command;
mod runtime_build;
mod setup;
pub(crate) mod tool_defs;

use crate::plugin::AgentPlugin;

pub(crate) use mode_execute::{AgentMode, resolve_agent_mode, should_enter_foreground};

pub use runtime_build::EngineConfigInterface;

pub use args::{extract_and_validate_session_flags, extract_tool_timeout, extract_tools_from_call};
pub use runtime_build::{extract_flag_config, resolve_config};

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
                "Log level for file-based logging (error|warn|info|debug|trace). Writes to $XDG_STATE_HOME/nu-agent/logs/agent.log",
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
        run_command::run_command(self, engine, call, input)
    }
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
mod tool_defs_test;

#[cfg(test)]
mod picker_test;

#[cfg(test)]
mod docs_contract_test;

#[cfg(test)]
mod resolve_policy_test;
