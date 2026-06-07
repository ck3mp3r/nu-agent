use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nu_plugin::EngineInterface;
use nu_protocol::{LabeledError, Span, Value};
use rig::client::CompletionClient;
use rig::memory::ConversationMemory;

use crate::{
    config::Config,
    plugin::RuntimeCtx,
    session::{
        CompactionInvocationMode, CompactionOutcome, CompactionStrategy, ConversationStore,
        JsonlConversationStore, Session, SessionStore, StoreEntry, extract_llm_context,
    },
    tools::{closure::ClosureRegistry, executor::ToolExecutor},
};

use crate::agent::{
    protocol::{
        compaction::{
            CompactionTriggerDecision, CompactionTriggerPolicy, CompactionTriggerSource,
            CompactionTriggerState, TwoTierCompactionPolicy,
        },
        contracts::{ConversationRuntime, McpUsabilityState, ProgressUi},
        event::UiEvent,
    },
    tools::{
        authz::{AsyncAskHook, PermissionsConfig, SessionGrantCache},
        handler::{self, McpToolRegistry},
    },
};
use crate::tools::mcp::{
    config::McpServerConfig,
    runtime::{McpRuntime, McpServerLifecycle},
};

const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

/// Build system preamble from components.
/// Joins non-empty parts with separators. Returns None if all empty.
fn build_system_preamble(
    config_preamble: Option<&str>,
    agent_persona: Option<&str>,
    sub_agent_instruction: Option<&str>,
    context: Option<&str>,
    agents_chain: Option<&str>,
    available_skills: Option<&str>,
) -> Option<String> {
    log::trace!(
        "build_system_preamble: config_preamble={}, agent_persona={}, sub_agent_instruction={}, context={}, agents_chain={}, available_skills={}",
        config_preamble.is_some(),
        agent_persona.is_some(),
        sub_agent_instruction.is_some(),
        context.is_some(),
        agents_chain.is_some(),
        available_skills.is_some()
    );

    let parts: Vec<&str> = [
        config_preamble,
        agent_persona,
        sub_agent_instruction,
        context,
        agents_chain,
        available_skills,
    ]
    .into_iter()
    .flatten()
    .collect();

    if parts.is_empty() {
        None
    } else {
        log::debug!(
            "build_system_preamble: parts_count={}, total_len={}",
            parts.len(),
            parts.iter().map(|p| p.len()).sum::<usize>()
        );
        Some(parts.join("\n\n---\n\n"))
    }
}

/// Apply tool filter patterns to a baseline set of tool definitions.
///
/// If `filter_patterns` is empty, returns a clone of the baseline (all tools visible).
/// Otherwise, returns only tools whose names match at least one glob pattern.
pub(crate) fn apply_tool_filter(
    baseline: &[rig::completion::ToolDefinition],
    filter_patterns: &[String],
) -> Vec<rig::completion::ToolDefinition> {
    if filter_patterns.is_empty() {
        return baseline.to_vec();
    }
    baseline
        .iter()
        .filter(|td| crate::tools::mcp::filter::matches_patterns(&td.name, filter_patterns))
        .cloned()
        .collect()
}

/// Build a shared HTTP client with sensible timeouts.
///
/// Connect timeout: 10s — fail fast if host is unreachable.
/// Request timeout: 120s — LLM responses can be slow, not infinite.
fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
        .expect("failed to build HTTP client")
}

/// Build a GitHub Copilot client using rig's from_env() or explicit config.
///
/// If config has an explicit `api_key`, uses the builder pattern with optional `base_url`.
/// Otherwise, delegates to `rig::providers::copilot::Client::from_env()` which handles
/// environment variable resolution (GITHUB_COPILOT_API_KEY → GITHUB_TOKEN → OAuth).
fn build_copilot_client(config: &Config) -> Result<rig::providers::copilot::Client, LabeledError> {
    let auth_err = |e: rig::http_client::Error| {
        LabeledError::new(format!(
            "Copilot auth failed: {e}. Run `agent auth login` to authenticate."
        ))
    };

    // Base URL: config takes precedence, then env vars (same as rig's from_env)
    let base_url = config
        .base_url
        .clone()
        .or_else(|| {
            std::env::var("GITHUB_COPILOT_API_BASE")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| {
            std::env::var("COPILOT_BASE_URL")
                .ok()
                .filter(|s| !s.trim().is_empty())
        });

    // 1. Explicit api_key from --api-key flag or plugin config
    if let Some(key) = &config.api_key {
        let mut b = rig::providers::copilot::Client::builder()
            .http_client(build_http_client());
        if let Some(url) = &base_url {
            b = b.base_url(url.clone());
        }
        return b
            .api_key::<rig::providers::copilot::CopilotAuth>(key.clone())
            .build()
            .map_err(auth_err);
    }

    // 2. GITHUB_COPILOT_API_KEY / COPILOT_API_KEY env var
    if let Ok(key) =
        std::env::var("GITHUB_COPILOT_API_KEY").or_else(|_| std::env::var("COPILOT_API_KEY"))
        && !key.trim().is_empty()
    {
        let mut b = rig::providers::copilot::Client::builder()
            .http_client(build_http_client());
        if let Some(url) = &base_url {
            b = b.base_url(url.clone());
        }
        return b
            .api_key::<rig::providers::copilot::CopilotAuth>(key)
            .build()
            .map_err(auth_err);
    }

    // 3. COPILOT_GITHUB_ACCESS_TOKEN / GITHUB_TOKEN env var
    if let Ok(token) =
        std::env::var("COPILOT_GITHUB_ACCESS_TOKEN").or_else(|_| std::env::var("GITHUB_TOKEN"))
        && !token.trim().is_empty()
    {
        let mut b = rig::providers::copilot::Client::builder()
            .http_client(build_http_client());
        if let Some(url) = &base_url {
            b = b.base_url(url.clone());
        }
        return b.github_access_token(token).build().map_err(auth_err);
    }

    // 4. Cached OAuth access token from prior `agent auth login`
    //    rig caches at: <config_dir>/github_copilot/access-token (plain text)
    let token_path = crate::utils::xdg::config_dir()
        .ok()
        .map(|d| d.join("github_copilot").join("access-token"));

    if let Some(path) = &token_path
        && let Ok(token) = std::fs::read_to_string(path)
        && !token.trim().is_empty()
    {
        let mut b = rig::providers::copilot::Client::builder()
            .http_client(build_http_client());
        if let Some(url) = &base_url {
            b = b.base_url(url.clone());
        }
        return b
            .github_access_token(token.trim())
            .build()
            .map_err(auth_err);
    }

    // 5. No auth available
    Err(LabeledError::new(
        "Not authenticated. Run `agent auth login` or set GITHUB_COPILOT_API_KEY / GITHUB_TOKEN environment variable.".to_string()
    ))
}

/// Build an OpenAI client using rig's builder pattern or from_env().
///
/// If config has an explicit `api_key`, uses the builder with optional `base_url`.
/// Otherwise, delegates to `rig::providers::openai::Client::from_env()` which reads
/// OPENAI_API_KEY environment variable.
fn build_openai_client(config: &Config) -> Result<rig::providers::openai::Client, LabeledError> {
    use rig::client::ProviderClient;

    let map_build_err = |e: rig::http_client::Error| {
        LabeledError::new(format!(
            "OpenAI client initialization failed: {e}. Ensure OPENAI_API_KEY is set."
        ))
    };

    let map_env_err = |e: rig::client::ProviderClientError| {
        LabeledError::new(format!(
            "OpenAI client initialization failed: {e}. Ensure OPENAI_API_KEY is set."
        ))
    };

    if let Some(key) = &config.api_key {
        let mut builder = rig::providers::openai::Client::builder()
            .http_client(build_http_client());
        if let Some(url) = &config.base_url {
            builder = builder.base_url(url.clone());
        }
        builder.api_key(key.clone()).build().map_err(map_build_err)
    } else {
        rig::providers::openai::Client::from_env().map_err(map_env_err)
    }
}

/// Build an Anthropic client using rig's builder pattern or from_env().
///
/// If config has an explicit `api_key`, uses the builder with optional `base_url`.
/// Otherwise, delegates to `rig::providers::anthropic::Client::from_env()` which reads
/// ANTHROPIC_API_KEY environment variable.
fn build_anthropic_client(
    config: &Config,
) -> Result<rig::providers::anthropic::Client, LabeledError> {
    use rig::client::ProviderClient;

    let map_build_err = |e: rig::http_client::Error| {
        LabeledError::new(format!(
            "Anthropic client initialization failed: {e}. Ensure ANTHROPIC_API_KEY is set."
        ))
    };

    let map_env_err = |e: rig::client::ProviderClientError| {
        LabeledError::new(format!(
            "Anthropic client initialization failed: {e}. Ensure ANTHROPIC_API_KEY is set."
        ))
    };

    if let Some(key) = &config.api_key {
        let mut builder = rig::providers::anthropic::Client::builder()
            .http_client(build_http_client());
        if let Some(url) = &config.base_url {
            builder = builder.base_url(url.clone());
        }
        builder.api_key(key.clone()).build().map_err(map_build_err)
    } else {
        rig::providers::anthropic::Client::from_env().map_err(map_env_err)
    }
}

/// Build an Ollama client.
///
/// Ollama doesn't require an API key. If `config.base_url` is set, uses the builder
/// with that URL. Otherwise falls back to `from_env()` which reads `OLLAMA_API_BASE_URL`
/// (defaults to http://localhost:11434).
fn build_ollama_client(config: &Config) -> Result<rig::providers::ollama::Client, LabeledError> {
    use rig::client::{Nothing, ProviderClient};

    let map_build_err = |e: rig::http_client::Error| {
        LabeledError::new(format!(
            "Ollama client initialization failed: {e}. Ensure Ollama is running."
        ))
    };

    let map_env_err = |e: rig::client::ProviderClientError| {
        LabeledError::new(format!(
            "Ollama client initialization failed: {e}. Ensure OLLAMA_API_BASE_URL is set or Ollama is running on default port."
        ))
    };

    if let Some(url) = &config.base_url {
        rig::providers::ollama::Client::builder()
            .http_client(build_http_client())
            .base_url(url.clone())
            .api_key(Nothing)
            .build()
            .map_err(map_build_err)
    } else {
        rig::providers::ollama::Client::from_env().map_err(map_env_err)
    }
}

/// Resolve which provider implementation to use.
/// If the provider config specifies an explicit `provider` field, use it.
/// Otherwise, the config key name itself is the provider type.
fn resolve_provider_type<'a>(provider_key: &'a str, provider_field: Option<&'a str>) -> &'a str {
    provider_field.unwrap_or(provider_key)
}

pub(crate) enum CachedProviderClient {
    Copilot(rig::providers::copilot::Client),
    OpenAi(rig::providers::openai::Client),
    Anthropic(rig::providers::anthropic::Client),
    Ollama(rig::providers::ollama::Client),
}

type ClientCacheKey = (String, Option<String>, Option<String>);

pub(crate) struct AgentConversationRuntime {
    pub runtime: tokio::runtime::Runtime,
    #[allow(dead_code)]
    pub runtime_ctx: RuntimeCtx,
    pub config: Config,
    pub tool_definitions: Vec<rig::completion::ToolDefinition>,
    pub baseline_tool_definitions: Vec<rig::completion::ToolDefinition>,
    pub closure_registry: ClosureRegistry,
    pub mcp_registry: McpToolRegistry,
    pub mcp_runtime: Option<McpRuntime>,
    pub mcp_tool_server_handle: rig::tool::server::ToolServerHandle,
    pub mcp_lifecycle_projection: Vec<McpServerLifecycle>,
    pub mcp_server_configs: Vec<McpServerConfig>,
    pub tool_filter_patterns: Vec<String>,
    pub mcp_caller_cwd: Option<std::path::PathBuf>,
    #[allow(dead_code)]
    pub tool_executor: ToolExecutor,
    pub engine: EngineInterface,
    pub store: SessionStore,
    pub final_session_id: Option<String>,
    pub compaction_threshold: Option<usize>,
    pub compaction_count: usize,
    pub auto_compaction_tolerance: usize,
    pub auto_compaction_hysteresis_margin: usize,
    pub auto_compaction_state: CompactionTriggerState,
    pub compaction_strategy: CompactionStrategy,
    pub compaction_proactive_threshold_pct: f64,
    pub compaction_fallback_strategies: Vec<CompactionStrategy>,
    pub startup_plugin_config: Option<crate::config::PluginConfig>,
    pub permissions: PermissionsConfig,
    pub permissions_startup_summary: String,
    pub permissions_startup_emitted: bool,
    pub session_grants: SessionGrantCache,
    pub ask_hook: AsyncAskHook,
    pub memory: rig::memory::InMemoryConversationMemory,
    pub conversation_store: JsonlConversationStore,
    pub memory_message_count: usize,
    pub memory_hydrated: bool,
    pub cached_client: Option<CachedProviderClient>,
    pub cached_client_key: Option<ClientCacheKey>,
    #[allow(dead_code)]
    pub agent_persona_body: Option<String>,
    #[allow(dead_code)]
    pub agent_identity: Option<String>,
    #[allow(dead_code)]
    pub agent_description: Option<String>,
    #[allow(dead_code)]
    pub cached_agents_chain: Option<String>,
    #[allow(dead_code)]
    pub cached_available_skills: Option<String>,
    #[allow(dead_code)]
    pub cached_sub_agent_instruction: Option<String>,
    #[allow(dead_code)]
    pub orchestrator: Option<
        std::sync::Arc<
            std::sync::Mutex<crate::agent::tools::handler::spawn_agent::OrchestratorState>,
        >,
    >,
    #[allow(dead_code)]
    pub broker_sender:
        Option<std::sync::Arc<tokio::sync::Mutex<crate::agent::mailbox::BrokerSender>>>,
    #[allow(dead_code)]
    pub mailbox_rx: Option<std::sync::mpsc::Receiver<crate::agent::mailbox::IncomingMessage>>,
    #[allow(dead_code)]
    pub parent_name: Option<String>,
    pub compacting: Arc<AtomicBool>,
    pub available_agent_summaries: Vec<crate::agent::protocol::persona::PersonaSummary>,
    pub agents_config: crate::config::AgentsConfig,
}

fn emit_permissions_startup_summary_once<U: ProgressUi>(
    ui: &mut U,
    emitted: &mut bool,
    summary: &str,
) {
    if !*emitted {
        ui.emit(&UiEvent::Warning {
            message: summary.to_string(),
        });
        *emitted = true;
    }
}

fn apply_switched_config(current: &mut Config, switched: Config) {
    *current = switched;
}

fn mcp_tool_definition_from_discovered(
    tool: &crate::tools::mcp::client::McpToolDefinition,
) -> rig::completion::ToolDefinition {
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
}

fn merge_new_mcp_tools_into_runtime(
    tool_definitions: &mut Vec<rig::completion::ToolDefinition>,
    mcp_registry: &mut McpToolRegistry,
    discovered_tools: &[crate::tools::mcp::client::McpToolDefinition],
    cli_patterns: &[String],
) -> Result<(), String> {
    let filtered =
        crate::tools::mcp::registration::registerable_tools(discovered_tools, cli_patterns);
    if filtered.is_empty() {
        return Ok(());
    }

    mcp_registry.register_tools(filtered.clone())?;

    let known_names = tool_definitions
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<std::collections::HashSet<_>>();

    for tool in filtered {
        if !known_names.contains(tool.name.as_str()) {
            tool_definitions.push(mcp_tool_definition_from_discovered(&tool));
        }
    }

    Ok(())
}

fn stage_enabled_mcp_runtime_state(
    current_tool_definitions: &[rig::completion::ToolDefinition],
    current_registry: &McpToolRegistry,
    server_name: &str,
    discovered_tools: &[crate::tools::mcp::client::McpToolDefinition],
    cli_patterns: &[String],
) -> Result<(Vec<rig::completion::ToolDefinition>, McpToolRegistry), String> {
    let mut staged_tool_definitions = current_tool_definitions.to_vec();
    let mut staged_registry = current_registry.clone();

    merge_new_mcp_tools_into_runtime(
        &mut staged_tool_definitions,
        &mut staged_registry,
        discovered_tools,
        cli_patterns,
    )?;
    staged_registry.set_server_enabled(server_name, true)?;

    Ok((staged_tool_definitions, staged_registry))
}

fn mcp_enable_runtime_config(
    mcp_server_configs: &[McpServerConfig],
    mcp_registry: &McpToolRegistry,
    server_to_enable: &str,
) -> Vec<McpServerConfig> {
    mcp_server_configs
        .iter()
        .map(|server| {
            let enable =
                server.name == server_to_enable || mcp_registry.is_server_enabled(&server.name);
            McpServerConfig {
                enabled: enable,
                ..server.clone()
            }
        })
        .collect()
}

fn rebuild_mcp_lifecycle_projection(
    mcp_runtime: Option<&McpRuntime>,
    mcp_server_configs: &[McpServerConfig],
    mcp_registry: &McpToolRegistry,
    tool_definitions: &[rig::completion::ToolDefinition],
) -> Vec<McpServerLifecycle> {
    let visible_count_by_server = tool_definitions
        .iter()
        .filter(|tool| mcp_registry.contains(tool.name.as_str()))
        .filter_map(|tool| {
            mcp_registry
                .server_name_for(tool.name.as_str())
                .map(str::to_string)
        })
        .fold(
            std::collections::HashMap::<String, usize>::new(),
            |mut acc, server| {
                *acc.entry(server).or_insert(0) += 1;
                acc
            },
        );

    let projected_runtime_config: Vec<McpServerConfig> = mcp_server_configs
        .iter()
        .map(|server| McpServerConfig {
            enabled: mcp_registry.is_server_enabled(&server.name),
            ..server.clone()
        })
        .collect();

    if let Some(runtime) = mcp_runtime {
        runtime
            .lifecycle_projection(&projected_runtime_config)
            .into_iter()
            .map(|mut lifecycle| {
                lifecycle.visible_tool_count = visible_count_by_server
                    .get(lifecycle.name.as_str())
                    .copied()
                    .unwrap_or(0);
                lifecycle
            })
            .collect()
    } else {
        let mut projection = projected_runtime_config
            .iter()
            .map(|server| McpServerLifecycle {
                name: server.name.clone(),
                configured: true,
                enabled: server.enabled,
                connected: false,
                visible_tool_count: visible_count_by_server
                    .get(server.name.as_str())
                    .copied()
                    .unwrap_or(0),
            })
            .collect::<Vec<_>>();
        projection.sort_by(|a, b| a.name.cmp(&b.name));
        projection
    }
}

impl ConversationRuntime for AgentConversationRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        if !enabled {
            self.mcp_registry.set_server_enabled(server_name, false)?;
            self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                self.mcp_runtime.as_ref(),
                &self.mcp_server_configs,
                &self.mcp_registry,
                &self.tool_definitions,
            );
            return Ok(McpUsabilityState::Disabled);
        }

        if !self
            .mcp_server_configs
            .iter()
            .any(|server| server.name == server_name)
        {
            self.mcp_registry.set_server_enabled(server_name, false)?;
            self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                self.mcp_runtime.as_ref(),
                &self.mcp_server_configs,
                &self.mcp_registry,
                &self.tool_definitions,
            );
            return Ok(McpUsabilityState::Failed);
        }

        let runtime_config =
            mcp_enable_runtime_config(&self.mcp_server_configs, &self.mcp_registry, server_name);

        match self
            .runtime
            .block_on(crate::tools::mcp::runtime::connect_servers(
                &runtime_config,
                self.mcp_caller_cwd.as_deref(),
            )) {
            Ok(runtime) if runtime.has_sessions() => {
                let discovered = runtime.discovered_tools().to_vec();

                let (staged_tool_definitions, staged_registry) = stage_enabled_mcp_runtime_state(
                    &self.tool_definitions,
                    &self.mcp_registry,
                    server_name,
                    &discovered,
                    &self.tool_filter_patterns,
                )?;

                self.tool_definitions = staged_tool_definitions;
                self.mcp_registry = staged_registry;
                self.mcp_runtime = Some(runtime);
                self.mcp_tool_server_handle = self
                    .mcp_runtime
                    .as_ref()
                    .map(McpRuntime::tool_server_handle)
                    .unwrap_or_else(|| rig::tool::server::ToolServer::new().run());
                self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                    self.mcp_runtime.as_ref(),
                    &self.mcp_server_configs,
                    &self.mcp_registry,
                    &self.tool_definitions,
                );

                Ok(McpUsabilityState::Enabled)
            }
            Ok(_) | Err(_) => {
                self.mcp_registry.set_server_enabled(server_name, false)?;
                self.mcp_lifecycle_projection = rebuild_mcp_lifecycle_projection(
                    self.mcp_runtime.as_ref(),
                    &self.mcp_server_configs,
                    &self.mcp_registry,
                    &self.tool_definitions,
                );
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

    fn llm_visible_mcp_tool_count_for_server(&self, server_name: &str) -> usize {
        self.active_tool_definitions()
            .iter()
            .filter(|tool| self.mcp_registry.is_registered(tool.name.as_str()))
            .filter_map(|tool| self.mcp_registry.server_name_for(tool.name.as_str()))
            .filter(|server| *server == server_name)
            .count()
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();

        for tool in self
            .active_tool_definitions()
            .iter()
            .filter(|tool| self.mcp_registry.is_registered(tool.name.as_str()))
        {
            let Some(server_name) = self.mcp_registry.server_name_for(tool.name.as_str()) else {
                continue;
            };
            grouped
                .entry(server_name.to_string())
                .or_default()
                .push(tool.name.clone());
        }

        grouped
            .into_iter()
            .map(|(server, mut names)| {
                names.sort();
                names.dedup();
                (server, names)
            })
            .collect()
    }

    fn switch_model(&mut self, model_spec: &str) -> Result<String, String> {
        let plugin_config = self.startup_plugin_config.clone().ok_or_else(|| {
            "model switch unavailable: startup plugin config cache is missing".to_string()
        })?;

        let resolved = plugin_config.resolve_model(model_spec)?;
        apply_switched_config(&mut self.config, resolved);
        self.cached_client = None;
        self.cached_client_key = None;
        Ok(format!("{}/{}", self.config.provider, self.config.model))
    }

    fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
        use crate::agent::protocol::persona::{
            FrontMatterParser, FsPersonaResolver, PersonaFileResolver,
            PulldownCmarkFrontMatterParser, interpret_front_matter,
        };

        let cwd = self
            .mcp_caller_cwd
            .clone()
            .ok_or_else(|| "agent switch unavailable: working directory not set".to_string())?;

        let config_dir = crate::utils::xdg::config_dir()
            .map(|base| base.join("nu-agent"))
            .map_err(|e| format!("agent switch failed: cannot determine config directory: {e}"))?;

        let resolver = FsPersonaResolver::new(cwd, config_dir, self.agents_config.clone());
        let (_path, contents) = resolver
            .resolve(agent_name)
            .map_err(|e| format!("agent switch failed: {e}"))?;

        let parser = PulldownCmarkFrontMatterParser;
        let raw = parser
            .parse(&contents)
            .map_err(|e| format!("agent switch failed: invalid front matter: {e}"))?;

        let parsed = interpret_front_matter(raw.front_matter.as_ref(), raw.body)
            .map_err(|e| format!("agent switch failed: invalid front matter fields: {e}"))?;

        // Update persona body
        self.agent_persona_body = Some(parsed.body);

        // Resolve identity: front matter name > agent_name argument
        let identity = parsed.name.unwrap_or_else(|| agent_name.to_string());
        self.agent_identity = Some(identity.clone());
        self.agent_description = parsed.description;

        // If persona specifies a model, attempt to switch (ignore errors)
        if let Some(ref model) = parsed.model {
            let _ = self.switch_model(model);
        }

        // Re-apply tool filter from new persona
        if let Some(ref filter_patterns) = parsed.tool_filter {
            self.tool_filter_patterns = filter_patterns.clone();
            self.tool_definitions =
                apply_tool_filter(&self.baseline_tool_definitions, filter_patterns);
        } else {
            self.tool_filter_patterns = Vec::new();
            self.tool_definitions = self.baseline_tool_definitions.clone();
        }

        // Invalidate cached client to pick up any changes
        self.cached_client = None;
        self.cached_client_key = None;

        log::debug!(
            "switch_agent: switched to identity={identity:?}, model={:?}, body_len={}",
            parsed.model,
            self.agent_persona_body.as_ref().map_or(0, |b| b.len())
        );

        Ok(identity)
    }

    fn active_model_identity(&self) -> String {
        format!("{}/{}", self.config.provider, self.config.model)
    }

    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        let Some(threshold) = self.compaction_threshold else {
            return Some(CompactionTriggerDecision::NoFire {
                reason: "signal_unavailable".to_string(),
            });
        };

        let policy = TwoTierCompactionPolicy::with_config(
            threshold,
            self.auto_compaction_tolerance,
            self.auto_compaction_hysteresis_margin,
            self.compaction_strategy,
            self.compaction_proactive_threshold_pct as f32,
            self.compaction_fallback_strategies.clone(),
        );
        Some(policy.evaluate(
            Some(self.memory_message_count),
            &mut self.auto_compaction_state,
        ))
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        self.execute_compaction_event(ui, source)
    }

    fn clear_session(&mut self) {
        self.memory = rig::memory::InMemoryConversationMemory::new();
        self.memory_message_count = 0;
        self.memory_hydrated = false;
    }

    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        use crate::agent::conversation::turn::{TurnContext, execute_turn};
        use crate::agent::hook::AuthzPermissionResolver;

        emit_permissions_startup_summary_once(
            ui,
            &mut self.permissions_startup_emitted,
            &self.permissions_startup_summary,
        );

        // Build system preamble from cached components
        let preamble = build_system_preamble(
            self.config.preamble.as_deref(),
            self.agent_persona_body.as_deref(),
            self.cached_sub_agent_instruction.as_deref(),
            context.as_deref(),
            self.cached_agents_chain.as_deref(),
            self.cached_available_skills.as_deref(),
        );

        log::debug!(
            "execute_turn: preamble present={}, preamble_len={}",
            preamble.is_some(),
            preamble.as_ref().map_or(0, |p| p.len())
        );

        // Hydrate memory from conversation store (idempotent, guarded)
        self.ensure_memory_hydrated()?;

        let conversation_id = if let Some(ref session_id) = self.final_session_id {
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

        // Provider dispatch - build model from cached client
        self.ensure_client_cached()?;

        // Macro to reduce TurnContext boilerplate
        macro_rules! run_with_model {
            ($model:expr) => {{
                let mut permission_resolver = AuthzPermissionResolver {
                    permissions: &self.permissions,
                    grant_cache: &mut self.session_grants,
                    ask_hook: &mut self.ask_hook,
                    engine: &self.engine,
                    closure_registry: &self.closure_registry,
                    mcp_registry: &self.mcp_registry,
                };
                execute_turn(
                    TurnContext {
                        runtime: self.runtime.handle(),
                        model: $model,
                        prompt: prompt.clone(),
                        memory: self.memory.clone(),
                        conversation_id,
                        preamble: preamble.as_deref(),
                        max_turns: self.config.max_tool_turns,
                        tool_server_handle: self.mcp_tool_server_handle.clone(),
                        closure_registry: &self.closure_registry,
                        mcp_registry: &self.mcp_registry,
                    },
                    ui,
                    &mut permission_resolver,
                )
            }};
        }

        macro_rules! with_cached_model {
            ($model_var:ident, $body:expr) => {
                match self.cached_client.as_ref().unwrap() {
                    CachedProviderClient::Copilot(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::OpenAi(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::Anthropic(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::Ollama(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                }
            };
        }

        let turn_result = match with_cached_model!(model, run_with_model!(model)) {
            Ok(result) => result,
            Err(e) if e.cancelled => {
                // Path A: rig hook cancelled — persist chat_history if available
                if let Some(ref session_id) = self.final_session_id
                    && let Some(ref messages) = e.messages
                {
                    if let Err(persist_err) = self.conversation_store.append(session_id, messages) {
                        log::warn!("Failed to persist cancelled turn messages: {}", persist_err);
                    }
                    self.memory_message_count += messages.len();
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
                return Ok(crate::llm::format_response(
                    &llm_response,
                    &self.config,
                    self.final_session_id.as_deref(),
                    self.compaction_count,
                    span,
                ));
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
            && let Some(ref session_id) = self.final_session_id
        {
            use rig::completion::Message;
            let mut cancelled_messages = vec![Message::user(prompt.clone())];
            if !turn_result.text.is_empty() {
                cancelled_messages.push(Message::assistant(turn_result.text.clone()));
            }
            if let Err(e) = self
                .conversation_store
                .append(session_id, &cancelled_messages)
            {
                log::warn!("Failed to persist cancelled turn messages (path B): {}", e);
            }
            self.memory_message_count += cancelled_messages.len();
        }

        // Persist new messages to conversation store if session exists
        if let Some(ref session_id) = self.final_session_id
            && let Some(ref messages) = turn_result.messages
        {
            // Persist the new messages from the turn result
            if let Err(e) = self.conversation_store.append(session_id, messages) {
                log::warn!(
                    "Failed to persist turn messages to conversation store: {}",
                    e
                );
            }

            // Update memory message count
            self.memory_message_count += messages.len();
        }

        // Format the response value
        let mut message_count = 0;
        let mut compaction_count = 0;

        if self.final_session_id.is_some() {
            match self.evaluate_auto_compaction() {
                Some(CompactionTriggerDecision::Fire { source, .. }) => {
                    if let Err(error) = self.execute_compaction_event(ui, source) {
                        ui.emit(&UiEvent::Warning { message: error });
                    }
                }
                Some(CompactionTriggerDecision::FallbackFire { source, .. }) => {
                    log::warn!(
                        "Compaction fallback triggered: executing with first available strategy"
                    );
                    if let Err(error) = self.execute_compaction_event(ui, source) {
                        ui.emit(&UiEvent::Warning { message: error });
                    }
                }
                _ => {}
            }

            message_count = self.memory_message_count;
            compaction_count = self.compaction_count;
        }

        // Only emit the full response if deltas weren't already emitted during streaming
        if !turn_result.deltas_emitted {
            ui.emit(&UiEvent::AssistantMessage {
                text: turn_result.text.clone(),
            });
        }
        ui.emit(&UiEvent::Completed {
            tool_calls: turn_result.tool_call_count,
        });
        ui.flush();

        // Build the response value with the same structure as the old path
        let llm_response = crate::llm::LlmResponse {
            text: turn_result.text,
            usage: crate::llm::LlmUsage {
                input_tokens: turn_result.usage.input_tokens,
                output_tokens: turn_result.usage.output_tokens,
                total_tokens: turn_result.usage.total_tokens,
                cached_input_tokens: turn_result.usage.cached_input_tokens,
                cache_creation_input_tokens: turn_result.usage.cache_creation_input_tokens,
            },
            tool_calls: Vec::new(), // TODO: track tool calls in TurnResult
            tool_call_metadata: Vec::new(), // TODO: track tool metadata in TurnResult
        };

        let response_value = crate::llm::format_response(
            &llm_response,
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

/// RAII guard that resets the compaction flag when dropped, even on error/panic.
struct CompactionGuard(Arc<AtomicBool>);

impl Drop for CompactionGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl AgentConversationRuntime {
    /// Idempotent memory hydration: loads stored messages into in-memory
    /// conversation memory exactly once per runtime lifetime (or until
    /// `clear_session` resets the guard).
    ///
    /// This must be called before any operation that reads memory
    /// (`execute_turn`, `execute_compaction_event`) so that reloaded
    /// sessions have their history available.
    fn ensure_memory_hydrated(&mut self) -> Result<(), LabeledError> {
        if self.memory_hydrated {
            return Ok(());
        }
        if let Some(ref session_id) = self.final_session_id {
            // Load ALL entries (messages + markers)
            let entries = self
                .conversation_store
                .load_all(session_id)
                .map_err(|e| LabeledError::new(format!("Failed to load session entries: {}", e)))?;

            // Extract only LLM-relevant messages (from latest marker onward)
            let llm_context = extract_llm_context(&entries);

            if !llm_context.is_empty() {
                self.runtime
                    .block_on(self.memory.append(session_id, llm_context.clone()))
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to append messages to memory: {}", e))
                    })?;
            }
            self.memory_message_count = llm_context.len();

            // Derive compaction_count from markers
            let marker_count = entries
                .iter()
                .filter(|e| matches!(e, StoreEntry::Marker(_)))
                .count();
            self.compaction_count = marker_count;
        }
        self.memory_hydrated = true;
        Ok(())
    }

    fn client_cache_key(&self) -> ClientCacheKey {
        (
            self.config.provider.clone(),
            self.config.api_key.clone(),
            self.config.base_url.clone(),
        )
    }

    fn ensure_client_cached(&mut self) -> Result<(), LabeledError> {
        let key = self.client_cache_key();
        if self.cached_client_key.as_ref() == Some(&key) {
            return Ok(());
        }
        let provider_key = self.config.provider.as_str();
        let provider_type = resolve_provider_type(
            provider_key,
            self.config.provider_impl.as_deref(),
        );
        log::info!(
            "creating {} client (type={}) for model={}",
            provider_key, provider_type, self.config.model
        );
        let client = match provider_type {
            "copilot" | "github-copilot" | "github_copilot" => {
                CachedProviderClient::Copilot(build_copilot_client(&self.config)?)
            }
            "openai" => CachedProviderClient::OpenAi(build_openai_client(&self.config)?),
            "anthropic" => CachedProviderClient::Anthropic(build_anthropic_client(&self.config)?),
            "ollama" => CachedProviderClient::Ollama(build_ollama_client(&self.config)?),
            other => {
                return Err(
                    LabeledError::new(format!(
                        "Unsupported provider: '{}' (from config key '{}')",
                        other, provider_key
                    ))
                    .with_help(
                        "Supported: copilot, openai, anthropic, ollama. \
                         Set 'provider' field in provider config to map custom names."
                    ),
                );
            }
        };
        self.cached_client = Some(client);
        self.cached_client_key = Some(key);
        Ok(())
    }

    fn active_tool_definitions(&self) -> Vec<rig::completion::ToolDefinition> {
        handler::llm_visible_tool_definitions(&self.tool_definitions, &self.mcp_registry)
    }

    fn execute_compaction_event<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        // Prevent overlapping compactions: if another is already running, skip.
        if self
            .compacting
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Ok(());
        }
        let _guard = CompactionGuard(Arc::clone(&self.compacting));

        self.ensure_client_cached().map_err(|e| e.to_string())?;

        // Ensure memory is hydrated before compaction reads it
        self.ensure_memory_hydrated().map_err(|e| e.to_string())?;

        let runtime = &self.runtime;
        let memory = &self.memory;
        let conversation_store = &self.conversation_store;
        let store = &self.store;

        let source_label = source.as_str().to_string();
        ui.emit(&UiEvent::CompactionStarted {
            source: source_label.clone(),
        });

        // Load session temporarily for compaction
        let session_id = self
            .final_session_id
            .as_ref()
            .ok_or_else(|| "session_unavailable".to_string())?;

        let mut session = store
            .load_session(session_id)
            .map_err(|e| format!("Failed to load session for compaction: {}", e))?;

        // Macro to reduce boilerplate for compaction
        macro_rules! compact_with_model {
            ($model:expr) => {{
                execute_compaction_event_shared(source, || {
                    let mode = match source {
                        CompactionTriggerSource::SlashCompact => CompactionInvocationMode::Force,
                        CompactionTriggerSource::AutoThreshold => {
                            CompactionInvocationMode::Threshold
                        }
                    };
                    runtime.block_on(execute_compaction(
                        &mut session,
                        memory,
                        conversation_store,
                        $model.clone(),
                        mode,
                        ui,
                        &source_label,
                    ))
                })
            }};
        }

        macro_rules! with_cached_model {
            ($model_var:ident, $body:expr) => {
                match self.cached_client.as_ref().unwrap() {
                    CachedProviderClient::Copilot(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::OpenAi(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::Anthropic(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                    CachedProviderClient::Ollama(c) => {
                        let $model_var = c.completion_model(&self.config.model);
                        $body
                    }
                }
            };
        }

        let result = with_cached_model!(model, compact_with_model!(model));

        match result {
            Ok(event) => {
                ui.emit(&event);

                // Update memory_message_count and compaction_count after successful compaction
                if let UiEvent::CompactionTriggered {
                    kept_recent_count, ..
                } = &event
                {
                    // After compaction: summary + kept_recent_count messages
                    self.memory_message_count = kept_recent_count + 1;
                    // Increment compaction count
                    self.compaction_count = session.compaction_count();
                }

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

/// Execute compaction using rig memory and ConversationStore.
///
/// This async function:
/// 1. Loads messages from InMemoryConversationMemory
/// 2. Calls the summarizer with old rig messages
/// 3. Compacts using `Session::compact`
/// 4. Updates memory and persists to store
///
/// # Arguments
/// * `runtime` - Tokio runtime for async operations
/// * `session` - Session to compact
/// * `memory` - InMemoryConversationMemory containing messages
/// * `store` - ConversationStore for persistence
/// * `summarizer` - Function that takes rig messages and returns summary
/// * `mode` - Compaction invocation mode (Threshold or Force)
///
/// # Returns
/// Ok(Some(outcome)) on successful compaction, Ok(None) if no compaction needed
async fn execute_compaction<M, S, U>(
    session: &mut Session,
    memory: &rig::memory::InMemoryConversationMemory,
    store: &S,
    model: M,
    mode: CompactionInvocationMode,
    ui: &mut U,
    source: &str,
) -> Result<Option<CompactionOutcome>, String>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    S: ConversationStore,
    U: ProgressUi,
{
    use rig::memory::ConversationMemory;

    // Load messages from memory to check threshold
    let messages = memory
        .load(session.id())
        .await
        .map_err(|e| format!("Failed to load messages from memory: {}", e))?;

    // Determine if compaction should run
    let should_compact = match mode {
        CompactionInvocationMode::Threshold => {
            messages.len() > session.config().compaction_threshold
        }
        CompactionInvocationMode::Force => true,
    };

    if !should_compact {
        return Ok(None);
    }

    // Perform compaction with summarizer closure
    let source_owned = source.to_string();
    let summarizer = |old_messages: &[rig::completion::Message]| {
        let messages = old_messages.to_vec();
        let model_clone = model.clone();
        let src = source_owned.clone();
        async move { summarize_messages(model_clone, ui, &messages, &src).await }
    };

    let outcome = session
        .compact(memory, store, summarizer)
        .await
        .map_err(|_| COMPACTION_FAILURE_WARNING.to_string())?;

    if outcome.summarized_count == 0 {
        return Ok(None);
    }

    Ok(Some(outcome))
}

fn summary_preview_text(summary_body: &str) -> String {
    let one_line = summary_body.replace('\n', " ");
    one_line.chars().take(120).collect()
}

/// Format rig messages for summarization.
///
/// Extracts text content from rig::completion::Message variants:
/// - Message::User { content } -> text from UserContent::Text
/// - Message::Assistant { content } -> text from AssistantContent::Text  
/// - Message::System { content } -> content string
///
/// Returns formatted string with role: content pairs.
fn format_messages_for_summary(messages: &[rig::completion::Message]) -> String {
    use rig::completion::message::{AssistantContent, UserContent};

    messages
        .iter()
        .map(|msg| {
            let role = match msg {
                rig::completion::Message::User { .. } => "user",
                rig::completion::Message::Assistant { .. } => "assistant",
                rig::completion::Message::System { .. } => "system",
            };

            let content = match msg {
                rig::completion::Message::User { content } => {
                    // Extract text from OneOrMany<UserContent>
                    content
                        .iter()
                        .filter_map(|c| match c {
                            UserContent::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                rig::completion::Message::Assistant { content, .. } => {
                    // Extract text from OneOrMany<AssistantContent>
                    content
                        .iter()
                        .filter_map(|c| match c {
                            AssistantContent::Text(text) => Some(text.text.as_str()),
                            AssistantContent::ToolCall(_) => None,
                            AssistantContent::Reasoning(_) => None,
                            AssistantContent::Image(_) => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                rig::completion::Message::System { content } => content.clone(),
            };

            format!("{}: {}", role, content)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Summarize old rig messages with LLM.
///
/// Formats rig messages, creates summarization prompt, and calls rig agent completion.
/// Uses streaming API to emit progressive chunks via `UiEvent::CompactionSummaryChunk`.
async fn summarize_messages<M, U>(
    model: M,
    ui: &mut U,
    old_messages: &[rig::completion::Message],
    source: &str,
) -> std::io::Result<String>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    U: ProgressUi,
{
    use futures::StreamExt;
    use rig::completion::Completion;

    let history = format_messages_for_summary(old_messages);
    let prompt_text = format!(
        "Summarize the following prior conversation segment concisely while preserving critical decisions, constraints, and open tasks.\n\n{}",
        history
    );

    // Build rig agent from model
    let agent = rig::agent::AgentBuilder::new(model).build();

    let stream_result = agent
        .completion(&prompt_text, Vec::<rig::completion::Message>::new())
        .await
        .map_err(|e| std::io::Error::other(format!("{}", e)))?
        .tools(vec![])
        .stream()
        .await
        .map_err(|e| std::io::Error::other(format!("{}", e)))?;

    let mut stream = std::pin::pin!(stream_result);
    let mut aggregated = String::new();

    loop {
        if ui.take_cancel_requested() {
            return Err(std::io::Error::other("Compaction cancelled by user"));
        }

        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(Ok(chunk)) => {
                        if let rig::streaming::StreamedAssistantContent::Text(delta) = chunk {
                            aggregated.push_str(&delta.text);
                            ui.emit(&UiEvent::CompactionSummaryChunk {
                                source: source.to_string(),
                                delta: delta.text,
                                aggregated: aggregated.clone(),
                            });
                        }
                    }
                    Some(Err(_)) => {
                        return Err(std::io::Error::other(COMPACTION_FAILURE_WARNING));
                    }
                    None => {
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(80)) => {
                ui.emit(&UiEvent::Tick);
            }
        }
    }

    Ok(aggregated)
}

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
