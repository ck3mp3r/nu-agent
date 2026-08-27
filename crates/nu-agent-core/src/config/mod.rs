use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::compaction::CompactionStrategy;
use crate::session::StoreType;

pub mod defaults;
pub mod models_cache;
pub mod secrets;
pub mod toml_config;

pub use models_cache::{ModelSpec, ModelsCache, ProviderSpec};
pub use secrets::{Credential, SecretStore, SecretStoreError};
pub use toml_config::{TomlConfigError, config_path, load};

/// Model limits (context and output token limits)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelLimits {
    pub context: Option<u32>,
    pub output: Option<u32>,
}

/// Model-specific configuration
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model limits
    pub limit: Option<ModelLimits>,

    /// Model display name (optional, defaults to key)
    pub name: Option<String>,

    /// Temperature for this model
    pub temperature: Option<f64>,

    /// Optional preamble text for this model
    pub preamble: Option<String>,

    /// Whether this model supports tool calling
    pub tool_call: Option<bool>,
}

/// Provider-specific configuration
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider display name
    pub name: Option<String>,

    /// API key for the provider
    pub api_key: Option<String>,

    /// Base URL for the provider API
    pub base_url: Option<String>,

    /// Provider implementation to use (e.g., "openai" for github-copilot)
    pub provider: Option<String>,

    /// Optional preamble text for this provider
    pub preamble: Option<String>,

    /// Models available for this provider
    #[serde(default)]
    pub models: HashMap<String, ModelConfig>,
}

/// Per-role model configuration.
///
/// Each entry in `PluginConfig.models` maps a role label (e.g. "default", "heavy", "light")
/// to a `ModelRoleConfig` that specifies the model string and optional overrides for
/// that role's runtime behavior.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelRoleConfig {
    /// Model specification in `provider/model` format (e.g. "openai/gpt-4").
    #[serde(default)]
    pub model: String,

    /// Temperature for response generation (0.0 to 2.0).
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// Maximum context tokens (input + output). Drives compaction threshold.
    pub max_context_tokens: Option<u32>,
    /// Maximum output tokens.
    pub max_output_tokens: Option<u32>,
    /// Maximum tool execution turns.
    pub max_tool_turns: Option<u32>,
    /// Maximum bytes of a single tool result before truncation. None = 20_000. Some(0) = unlimited.
    pub max_tool_result_bytes: Option<usize>,
    /// Maximum tool calls allowed per sub-turn (single LLM response).
    pub max_tool_calls_per_subturn: Option<usize>,
    /// Approximate context window in tokens for the configured model. None = no warning.
    pub model_context_tokens: Option<usize>,
    /// Fraction of model_context_tokens at which to warn (0.0–1.0). None = 0.6.
    pub context_warning_threshold: Option<f32>,
    /// Additional provider-specific parameters forwarded verbatim to the completion request.
    pub additional_params: Option<serde_json::Value>,
    /// Read timeout for HTTP streaming responses in seconds (default: 30).
    pub read_timeout_secs: Option<u64>,
    /// Max retry attempts for transient errors. None = 3.
    pub max_retries: Option<u8>,
    /// Base backoff in ms, doubles each attempt, capped at 30_000ms. None = 1000.
    pub retry_base_delay_ms: Option<u64>,
}

/// Top-level plugin configuration (provider-centric)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PluginConfig {
    /// Model role map — maps role labels (e.g. "default", "heavy", "light")
    /// to per-role model configuration. At minimum must contain a "default" entry.
    #[serde(default)]
    pub models: HashMap<String, ModelRoleConfig>,

    /// Provider configurations
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,

    /// Compaction configuration (optional, uses defaults if not set)
    #[serde(default)]
    pub compaction: Option<CompactionConfig>,

    /// Agent persona configuration
    #[serde(default)]
    pub agents: AgentsConfig,

    /// Enable A2A agent-to-agent protocol support (experimental, default: false).
    pub a2a_enabled: Option<bool>,

    /// Session store backend configuration (optional, defaults to SQLite).
    #[serde(default)]
    pub session_store: Option<StoreTypeConfig>,

    /// Secret store for API keys and OAuth tokens (not serialized).
    #[serde(skip)]
    pub secret_store: Option<SecretStore>,

    /// Local models.dev cache (not serialized). Populated at runtime.
    #[serde(skip)]
    pub models_cache: Option<ModelsCache>,

    /// Tool permissions configuration (raw TOML table, parsed at runtime)
    #[serde(default)]
    pub permissions: Option<toml::Value>,

    /// MCP server configurations (raw TOML table, parsed at runtime)
    #[serde(default)]
    pub mcp: Option<toml::Value>,
}
/// All fields are `Option` — `None` means "use default".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Primary compaction strategy: "sliding_summary"
    pub strategy: Option<CompactionStrategy>,
    /// Proactive compaction threshold percentage 0.0-1.0 (default: 0.80)
    pub proactive_threshold_pct: Option<f64>,
}

/// Configuration for session store backend selection.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StoreTypeConfig {
    /// Session store backend type: sqlite or jsonl
    #[serde(default)]
    pub store_type: StoreType,
    /// Optional custom path for the session store
    pub path: Option<String>,
}

/// Configuration for agent personas (planner/maker).
///
/// Controls which built-in personas are available and which is the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentsConfig {
    /// Whether the planner persona is enabled
    #[serde(default = "default_true")]
    pub planner_enabled: bool,
    /// Whether the maker persona is enabled
    #[serde(default = "default_true")]
    pub maker_enabled: bool,
    /// Default persona at startup (e.g., "planner" or "maker")
    #[serde(default)]
    pub default: String,
    /// Fallback persona name (an .agents/*.md file) when default is disabled
    pub fallback: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            planner_enabled: true,
            maker_enabled: true,
            default: "planner".to_string(),
            fallback: None,
        }
    }
}

impl CompactionConfig {
    /// Validate the compaction configuration.
    ///
    /// Rules:
    /// - `proactive_threshold_pct` must be in 0.0..=1.0 if set
    pub fn validate(&self) -> Result<(), String> {
        if let Some(pct) = self.proactive_threshold_pct
            && !(0.0..=1.0).contains(&pct)
        {
            return Err(format!(
                "proactive_threshold_pct must be between 0.0 and 1.0, got {}",
                pct
            ));
        }

        Ok(())
    }
}

impl PluginConfig {
    /// Resolve a model role configuration to runtime Config.
    ///
    /// The `role_config.model` field must be in `"provider/model"` format:
    /// - Provider: extracted from first part before `/`
    /// - Model: everything after first `/` (may contain additional `/` characters)
    ///
    /// # Examples
    /// - `"openai/gpt-4"` → provider: `"openai"`, model: `"gpt-4"`
    /// - `"github-copilot/anthropic/claude-sonnet-4-20250514"` → provider: `"github-copilot"`, model: `"anthropic/claude-sonnet-4-20250514"`
    ///
    /// # Resolution order (last wins)
    /// 1. `Config::from_env(provider_name, model_name)` — env vars (lowest priority)
    /// 2. Provider config — `api_key`, `base_url`, `provider_impl`
    /// 3. Model config — `temperature`, `limits` (context/output) from `ProviderConfig.models.<name>`
    /// 4. Role config — all `ModelRoleConfig` fields override model config (highest priority)
    ///
    /// # Arguments
    /// * `role_config` - Per-role model configuration including the model spec and overrides
    ///
    /// # Returns
    /// Resolved Config with provider, model, and merged settings from all sources
    ///
    /// # Errors
    /// - Missing `/` separator in model spec
    /// - Empty provider or model name
    pub fn resolve_model(&self, role_config: &ModelRoleConfig) -> Result<Config, String> {
        let model_spec = &role_config.model;

        // Split on first '/' only - provider is first part, model is everything after
        let (provider_name, model_name) = model_spec.split_once('/').ok_or_else(|| {
            format!(
                "Invalid model specification '{}'. Expected 'provider/model' format",
                model_spec
            )
        })?;

        // Validate non-empty parts
        if provider_name.is_empty() {
            return Err("Provider name cannot be empty".to_string());
        }
        if model_name.is_empty() {
            return Err("Model name cannot be empty".to_string());
        }

        // Look up provider configuration (optional — provider block not required)
        let provider_config = self.providers.get(provider_name);
        let model_config = provider_config.and_then(|pc| pc.models.get(model_name));

        log::debug!(
            "resolve_model: spec={model_spec} provider={provider_name} model={model_name} config_found={}",
            model_config.is_some()
        );

        // Step 1: Start with env-based config for this provider/model (lowest priority)
        let mut config = Config::from_env(provider_name, model_name);

        // Step 2: Merge provider-level settings (if provider block exists)
        if let Some(pc) = provider_config
            && let Some(impl_name) = &pc.provider
        {
            config.provider_impl = Some(impl_name.clone());
        }
        if let Some(pc) = provider_config
            && let Some(api_key) = &pc.api_key
        {
            config.api_key = Some(api_key.clone());
        }
        if let Some(pc) = provider_config
            && let Some(base_url) = &pc.base_url
        {
            config.base_url = Some(base_url.clone());
        }

        // Step 3: Merge model-specific settings (if model exists in config)
        if let Some(model_cfg) = model_config {
            if let Some(temp) = model_cfg.temperature {
                config.temperature = Some(temp);
            }
            if let Some(limits) = &model_cfg.limit {
                if let Some(context) = limits.context {
                    config.max_context_tokens = Some(context);
                }
                if let Some(output) = limits.output {
                    config.max_output_tokens = Some(output);
                }
            }
        }

        // Step 4: Apply role-level config overrides (highest priority within resolve_model)
        if let Some(temp) = role_config.temperature {
            config.temperature = Some(temp);
        }
        if let Some(t) = role_config.max_tokens {
            config.max_tokens = Some(t);
        }
        if let Some(ctx) = role_config.max_context_tokens {
            config.max_context_tokens = Some(ctx);
        }
        if let Some(out) = role_config.max_output_tokens {
            config.max_output_tokens = Some(out);
        }
        if let Some(t) = role_config.max_tool_turns {
            config.max_tool_turns = Some(t);
        }
        if let Some(b) = role_config.max_tool_result_bytes {
            config.max_tool_result_bytes = Some(b);
        }
        if let Some(c) = role_config.max_tool_calls_per_subturn {
            config.max_tool_calls_per_subturn = Some(c);
        }
        if let Some(m) = role_config.model_context_tokens {
            config.model_context_tokens = Some(m);
        }
        if let Some(t) = role_config.context_warning_threshold {
            config.context_warning_threshold = Some(t);
        }
        if let Some(p) = &role_config.additional_params {
            config.additional_params = Some(p.clone());
        }
        if let Some(r) = role_config.read_timeout_secs {
            config.read_timeout_secs = Some(r);
        }
        if let Some(r) = role_config.max_retries {
            config.max_retries = Some(r);
        }
        if let Some(r) = role_config.retry_base_delay_ms {
            config.retry_base_delay_ms = Some(r);
        }

        // Resolve secret store references (e.g. "store:openai" → actual key)
        if let Some(ref store) = self.secret_store
            && let Some(ref key) = config.api_key
            && let Some(resolved) = store.resolve(key)
        {
            config.api_key = Some(resolved);
        }

        // Apply models.json cache specs (fill missing values only)
        if let Some(ref cache) = self.models_cache
            && let Some(spec) = cache.get_spec(provider_name, model_name)
        {
            if config.max_context_tokens.is_none() {
                config.max_context_tokens = Some(spec.limit.context);
            }
            if config.max_output_tokens.is_none() {
                config.max_output_tokens = Some(spec.limit.output);
            }
            if config.model_context_tokens.is_none() {
                config.model_context_tokens = Some(spec.limit.context as usize);
            }
        }

        // Forward global plugin config fields (not model-specific).
        // Only apply plugin config value when env var didn't set it.
        if config.a2a_enabled.is_none() {
            config.a2a_enabled = self.a2a_enabled;
        }

        // Forward session_store_type from plugin config (env var already checked)
        if config.session_store_type.is_none() {
            config.session_store_type = self.session_store.as_ref().map(|s| s.store_type);
        }

        Ok(config)
    }
}

/// Runtime configuration for a specific invocation
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    /// LLM provider (e.g., "openai", "anthropic", "copilot")
    pub provider: String,

    /// Provider implementation to use (e.g., "openai" for a github-copilot provider)
    /// If None, uses the provider name itself
    pub provider_impl: Option<String>,

    /// Model identifier (e.g., "gpt-4", "claude-3-opus")
    pub model: String,

    /// API key for the provider (if not set, will use environment variable)
    pub api_key: Option<String>,

    /// Base URL override for the provider API
    pub base_url: Option<String>,

    /// Temperature for response generation (0.0 to 2.0)
    pub temperature: Option<f64>,

    /// Maximum tokens to generate
    pub max_tokens: Option<u32>,

    /// Maximum context tokens (input + output)
    pub max_context_tokens: Option<u32>,

    /// Maximum output tokens
    pub max_output_tokens: Option<u32>,

    /// Maximum tool execution turns
    pub max_tool_turns: Option<u32>,

    /// Resolved system preamble to prepend before prompt/context
    pub preamble: Option<String>,

    /// Read timeout for HTTP streaming responses in seconds (default: 30).
    /// Only fires when no bytes are received for this duration — safe for long
    /// but active LLM responses. Set to 0 to disable. None = use default (30s).
    pub read_timeout_secs: Option<u64>,

    /// Maximum bytes of a single tool result before truncation.
    /// Full output saved to a temp file; LLM is told to use `read` with offset/limit.
    /// None = use runtime default (20_000). Some(0) = disable truncation.
    pub max_tool_result_bytes: Option<usize>,

    /// Approximate context window in tokens for the configured model.
    /// None = no warning. Set explicitly — no per-provider auto-detection.
    /// Reference: claude-sonnet-4 = 200_000, gpt-4o = 128_000.
    pub model_context_tokens: Option<usize>,

    /// Fraction of model_context_tokens at which to warn (0.0–1.0).
    /// Default: 0.6 — conservative to compensate for the chars/4 under-count.
    /// None falls back to 0.6.
    pub context_warning_threshold: Option<f32>,

    /// Max retry attempts for transient errors. None = use default (3).
    pub max_retries: Option<u8>,

    /// Base backoff in ms, doubles each attempt, capped at 30_000ms. None = use default (1000).
    pub retry_base_delay_ms: Option<u64>,

    /// Maximum tool calls allowed per sub-turn (single LLM response).
    /// Defense against models that ignore `parallel_tool_calls: false` and emit
    /// many tool calls in one response, causing oversized follow-up requests.
    /// None = use default (10). Some(0) = unlimited.
    pub max_tool_calls_per_subturn: Option<usize>,

    /// Additional provider-specific parameters forwarded verbatim to the
    /// completion request. Merged as a JSON object into the request body.
    /// Example: `{ thinking: { type: "disabled" } }` disables Anthropic extended thinking.
    /// None = no additional parameters.
    pub additional_params: Option<serde_json::Value>,

    /// Enable A2A agent-to-agent protocol support (experimental, default: false).
    pub a2a_enabled: Option<bool>,

    /// A2A agent port (CLI-only). Some(0) = random, Some(n>0) = fixed, None = not set.
    pub a2a_port: Option<u16>,

    /// Session store backend type. None = use default (SQLite).
    pub session_store_type: Option<StoreType>,
}

impl Config {
    /// Returns max_context_tokens, falling back to defaults::MAX_CONTEXT_TOKENS.
    pub fn resolved_max_context_tokens(&self) -> u32 {
        self.max_context_tokens
            .unwrap_or(defaults::MAX_CONTEXT_TOKENS)
    }

    /// Create a Config by reading environment variables.
    ///
    /// Looks for:
    /// - `{PROVIDER}_API_KEY` (e.g., `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`)
    /// - Special fallback for "copilot" provider: `GITHUB_COPILOT_API_KEY` → `GITHUB_TOKEN`
    /// - `AGENT_TEMPERATURE`, `AGENT_MAX_TOKENS`, etc. for overrides
    ///
    /// Invalid values are gracefully ignored (set to None).
    pub fn from_env(provider: &str, model: &str) -> Self {
        use std::env;

        // Helper to parse environment variable with error handling
        fn parse_env_var<T: std::str::FromStr>(key: &str) -> Option<T> {
            env::var(key).ok().and_then(|val| val.parse().ok())
        }

        // Provider-specific API key
        // For copilot, let rig's from_env() handle environment variable resolution
        let api_key = if provider.eq_ignore_ascii_case("copilot")
            || provider.eq_ignore_ascii_case("github-copilot")
        {
            // For copilot providers, don't resolve env vars here
            // rig's from_env() handles GITHUB_COPILOT_API_KEY → GITHUB_TOKEN → OAuth
            None
        } else {
            // Standard provider-specific API key (e.g., OPENAI_API_KEY)
            let provider_upper = provider.to_uppercase();
            let api_key_var = format!("{}_API_KEY", provider_upper);
            env::var(&api_key_var).ok()
        };

        // AGENT_* overrides
        let base_url = env::var("AGENT_BASE_URL").ok();
        let temperature = parse_env_var("AGENT_TEMPERATURE");
        let max_tokens = parse_env_var("AGENT_MAX_TOKENS");
        let max_context_tokens = parse_env_var("AGENT_MAX_CONTEXT_TOKENS");
        let max_output_tokens = parse_env_var("AGENT_MAX_OUTPUT_TOKENS");
        let max_tool_turns = parse_env_var("AGENT_MAX_TOOL_TURNS"); // No default - runtime decides based on mode
        let max_tool_result_bytes = parse_env_var("AGENT_MAX_TOOL_RESULT_BYTES");
        let model_context_tokens = parse_env_var("AGENT_MODEL_CONTEXT_TOKENS");
        let context_warning_threshold = parse_env_var("AGENT_CONTEXT_WARNING_THRESHOLD");
        let max_tool_calls_per_subturn = parse_env_var("AGENT_MAX_TOOL_CALLS_PER_SUBTURN");
        let max_retries: Option<u8> = parse_env_var("AGENT_MAX_RETRIES");
        let retry_base_delay_ms: Option<u64> = parse_env_var("AGENT_RETRY_BASE_DELAY_MS");
        let read_timeout_secs: Option<u64> = parse_env_var("AGENT_READ_TIMEOUT_SECS");
        let a2a_enabled: Option<bool> = parse_env_var("AGENT_A2A_ENABLED");

        let a2a_port: Option<u16> = parse_env_var("AGENT_A2A_PORT");

        let session_store_type: Option<StoreType> = env::var("AGENT_SESSION_STORE_TYPE")
            .ok()
            .and_then(|s| s.parse().ok());

        log::debug!(
            "Config.from_env: provider={provider} model={model} api_key={} base_url={base_url:?}",
            api_key.is_some()
        );

        Self {
            provider: provider.to_string(),
            provider_impl: None, // from_env doesn't use provider_impl
            model: model.to_string(),
            api_key,
            base_url,
            temperature,
            max_tokens,
            max_context_tokens,
            max_output_tokens,
            max_tool_turns,
            preamble: None,
            read_timeout_secs,
            max_tool_result_bytes,
            model_context_tokens,
            context_warning_threshold,
            max_retries,
            retry_base_delay_ms,
            max_tool_calls_per_subturn,
            additional_params: None,
            a2a_enabled,
            a2a_port,
            session_store_type,
        }
    }

    /// Validate the configuration according to MVP rules.
    ///
    /// Validation rules:
    /// 1. Provider must be a non-empty string
    /// 2. Model must be a non-empty string
    /// 3. If both max_output_tokens and max_context_tokens are set,
    ///    max_output_tokens must be <= max_context_tokens
    /// 4. If max_tool_turns is set, it must be > 0
    ///
    /// Returns Ok(()) if valid, or Err with descriptive message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        // Rule 1: Provider must be non-empty
        if self.provider.is_empty() {
            return Err("provider must be a non-empty string".to_string());
        }

        // Rule 2: Model must be non-empty
        if self.model.is_empty() {
            return Err("model must be a non-empty string".to_string());
        }

        // Rule 3: max_output_tokens <= max_context_tokens (if both provided)
        if let (Some(output), Some(context)) = (self.max_output_tokens, self.max_context_tokens)
            && output > context
        {
            return Err(format!(
                "max_output_tokens ({}) must be <= max_context_tokens ({})",
                output, context
            ));
        }

        // Rule 4: max_tool_turns > 0 (if provided)
        if let Some(turns) = self.max_tool_turns
            && turns == 0
        {
            return Err("max_tool_turns must be greater than 0".to_string());
        }

        // Rule 5: context_warning_threshold must be in (0.0, 1.0] if set
        if let Some(threshold) = self.context_warning_threshold
            && (threshold <= 0.0 || threshold > 1.0)
        {
            return Err(format!(
                "context_warning_threshold must be in (0.0, 1.0], got {}",
                threshold
            ));
        }

        // Rule 6: model_context_tokens must be > 0 if set (guards divide-by-zero)
        if let Some(limit) = self.model_context_tokens
            && limit == 0
        {
            return Err("model_context_tokens must be greater than 0".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod test;
