use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::compaction::CompactionStrategy;
use crate::session::StoreType;

pub mod defaults;

/// Model limits (context and output token limits)
#[derive(Debug, Clone, PartialEq)]
pub struct ModelLimits {
    pub context: Option<u32>,
    pub output: Option<u32>,
}

/// Model-specific configuration
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
    pub models: HashMap<String, ModelConfig>,
}

/// Per-role model configuration.
///
/// Each entry in `PluginConfig.models` maps a role label (e.g. "default", "heavy", "light")
/// to a `ModelRoleConfig` that specifies the model string and optional overrides for
/// that role's runtime behavior.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelRoleConfig {
    /// Model specification in `provider/model` format (e.g. "openai/gpt-4").
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
#[derive(Debug, Clone, PartialEq)]
pub struct PluginConfig {
    /// Model role map — maps role labels (e.g. "default", "heavy", "light")
    /// to per-role model configuration. At minimum must contain a "default" entry.
    pub models: HashMap<String, ModelRoleConfig>,

    /// Provider configurations
    pub providers: HashMap<String, ProviderConfig>,

    /// Compaction configuration (optional, uses defaults if not set)
    pub compaction: Option<CompactionConfig>,

    /// Agent persona configuration
    pub agents: AgentsConfig,

    /// Enable A2A agent-to-agent protocol support (experimental, default: false).
    pub a2a_enabled: bool,

    /// Session store backend configuration (optional, defaults to SQLite).
    pub session_store: Option<StoreTypeConfig>,
}

/// Configuration for conversation compaction behavior.
///
/// All fields are `Option` — `None` means "use default". This follows
/// the merge pattern from `Config::merge()`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Primary compaction strategy: "sliding_summary", "sliding_window", "token_truncate"
    pub strategy: Option<CompactionStrategy>,
    /// Number of recent messages to keep during compaction (default: 10)
    pub keep_recent: Option<usize>,
    /// Token budget for TokenTruncate strategy (chars/4 estimation)
    pub token_budget: Option<usize>,
    /// Proactive compaction threshold percentage 0.0-1.0 (default: 0.80)
    pub proactive_threshold_pct: Option<f64>,
}

/// Configuration for session store backend selection.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreTypeConfig {
    /// Session store backend type: sqlite or jsonl
    pub store_type: StoreType,
    /// Optional custom path for the session store
    pub path: Option<String>,
}

/// Configuration for agent personas (planner/maker).
///
/// Controls which built-in personas are available and which is the default.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentsConfig {
    /// Whether the planner persona is enabled
    pub planner_enabled: bool,
    /// Whether the maker persona is enabled
    pub maker_enabled: bool,
    /// Default persona at startup (e.g., "planner" or "maker")
    pub default: String,
    /// Fallback persona name (an .agents/*.md file) when default is disabled
    pub fallback: Option<String>,
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
    /// - `keep_recent` must be > 0 if set
    pub fn validate(&self) -> Result<(), String> {
        if let Some(pct) = self.proactive_threshold_pct
            && !(0.0..=1.0).contains(&pct)
        {
            return Err(format!(
                "proactive_threshold_pct must be between 0.0 and 1.0, got {}",
                pct
            ));
        }

        if let Some(keep_recent) = self.keep_recent
            && keep_recent == 0
        {
            return Err("keep_recent must be greater than 0".to_string());
        }

        if let Some(CompactionStrategy::TokenTruncate) = self.strategy
            && self.token_budget.is_none()
        {
            return Err("token_budget must be set when using token_truncate strategy".to_string());
        }

        Ok(())
    }
}

impl PluginConfig {
    /// Parse PluginConfig from Nushell record
    ///
    /// Expected structure:
    /// ```nushell
    /// {
    ///   models: {
    ///     default: {
    ///       model: "openai/gpt-4"
    ///       temperature: 0.7          # optional
    ///       max_tokens: 2048          # optional
    ///       max_context_tokens: 32000 # optional
    ///       max_output_tokens: 1024   # optional
    ///       max_tool_turns: 5         # optional
    ///       read_timeout_secs: 60     # optional
    ///     }
    ///     heavy: { model: "openai/gpt-4-turbo" }   # optional
    ///     light: { model: "openai/gpt-3.5-turbo" } # optional
    ///   }
    ///   providers: {
    ///     openai: {
    ///       name: "OpenAI"  # optional
    ///       api_key: "sk-..."  # optional
    ///       base_url: "https://..."  # optional
    ///       provider: "openai"  # optional, for custom providers
    ///       models: {
    ///         "gpt-4": {
    ///           name: "GPT-4"  # optional
    ///           temperature: 0.7  # optional
    ///           tool_call: true  # optional
    ///           limit: {  # optional
    ///             context: 128000
    ///             output: 4096
    ///           }
    ///         }
    ///       }
    ///     }
    ///   }
    /// }
    /// ```
    pub fn from_plugin_config(
        value: &nu_protocol::Value,
    ) -> Result<Self, nu_protocol::LabeledError> {
        use nu_protocol::LabeledError;

        // Helper to create labeled error
        fn labeled_error(msg: &str, label: &str, span: nu_protocol::Span) -> LabeledError {
            LabeledError::new(msg).with_label(label, span)
        }

        // Ensure value is a record
        let record = value.as_record().map_err(|_| {
            labeled_error(
                "Invalid plugin configuration",
                "Expected a record for plugin configuration",
                value.span(),
            )
        })?;

        let span = value.span();

        // Extract required 'models' field — a record mapping role labels to model role configs
        let models_record = record
            .get("models")
            .ok_or_else(|| {
                labeled_error(
                    "Missing required field 'models'",
                    "Missing 'models' field",
                    span,
                )
            })?
            .as_record()
            .map_err(|_| {
                labeled_error(
                    "'models' must be a record",
                    "'models' must be a record",
                    span,
                )
            })?;

        let mut models = HashMap::new();
        for (key, value) in models_record.iter() {
            let role_record = value.as_record().map_err(|_| {
                labeled_error(
                    &format!("'models.{key}' must be a record"),
                    &format!("'models.{key}' must be a record"),
                    span,
                )
            })?;

            // Extract required 'model' field
            let model_str = role_record
                .get("model")
                .ok_or_else(|| {
                    labeled_error(
                        &format!("'models.{key}.model' is required"),
                        &format!("'models.{key}.model' is required"),
                        span,
                    )
                })?
                .as_str()
                .map_err(|_| {
                    labeled_error(
                        &format!("'models.{key}.model' must be a string"),
                        &format!("'models.{key}.model' must be a string"),
                        span,
                    )
                })?;

            // Validate provider/model format (must contain '/')
            if !model_str.contains('/') {
                return Err(labeled_error(
                    &format!(
                        "models.{key}.model must be in provider/model format, got '{model_str}'"
                    ),
                    &format!(
                        "models.{key}.model must be in provider/model format, got '{model_str}'"
                    ),
                    span,
                ));
            }

            // Parse optional fields
            let temperature = role_record
                .get("temperature")
                .and_then(|v| v.as_float().ok());
            let max_tokens = role_record.get("max_tokens").and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
            });
            let max_context_tokens = role_record.get("max_context_tokens").and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
            });
            let max_output_tokens = role_record.get("max_output_tokens").and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
            });
            let max_tool_turns = role_record.get("max_tool_turns").and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
            });
            let max_tool_result_bytes = role_record.get("max_tool_result_bytes").and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as usize) } else { None })
            });
            let max_tool_calls_per_subturn =
                role_record.get("max_tool_calls_per_subturn").and_then(|v| {
                    v.as_int()
                        .ok()
                        .and_then(|i| if i >= 0 { Some(i as usize) } else { None })
                });
            let model_context_tokens = role_record.get("model_context_tokens").and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as usize) } else { None })
            });
            let context_warning_threshold = role_record
                .get("context_warning_threshold")
                .and_then(|v| v.as_float().ok())
                .map(|f| f as f32);
            let additional_params = role_record
                .get("additional_params")
                .map(nu_value_to_json)
                .transpose()
                .map_err(|e| {
                    nu_protocol::LabeledError::new(format!(
                        "Invalid models.{key}.additional_params: {e}"
                    ))
                    .with_label("expected a record", span)
                })?;
            let read_timeout_secs = role_record.get("read_timeout_secs").and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as u64) } else { None })
            });
            let max_retries = role_record.get("max_retries").and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as u8) } else { None })
            });
            let retry_base_delay_ms = role_record.get("retry_base_delay_ms").and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as u64) } else { None })
            });

            models.insert(
                key.clone(),
                ModelRoleConfig {
                    model: model_str.to_string(),
                    temperature,
                    max_tokens,
                    max_context_tokens,
                    max_output_tokens,
                    max_tool_turns,
                    max_tool_result_bytes,
                    max_tool_calls_per_subturn,
                    model_context_tokens,
                    context_warning_threshold,
                    additional_params,
                    read_timeout_secs,
                    max_retries,
                    retry_base_delay_ms,
                },
            );
        }

        // Validate that 'default' role exists
        if !models.contains_key("default") {
            return Err(labeled_error(
                "models.default is required",
                "models.default is required",
                span,
            ));
        }

        // Extract required 'providers' field
        let providers_record = record
            .get("providers")
            .ok_or_else(|| {
                labeled_error("Missing required field", "Missing 'providers' field", span)
            })?
            .as_record()
            .map_err(|_| {
                labeled_error("Invalid field type", "'providers' must be a record", span)
            })?;

        // Parse each provider
        let mut providers = HashMap::new();
        for (provider_name, provider_value) in providers_record.iter() {
            let provider_config = Self::parse_provider_config(provider_value)?;
            providers.insert(provider_name.clone(), provider_config);
        }

        // Extract optional 'compaction' config
        let compaction = if let Some(compaction_value) = record.get("compaction") {
            Some(Self::parse_compaction_config(compaction_value)?)
        } else {
            None
        };

        // Extract optional 'agents' config
        let agents = if let Some(agents_value) = record.get("agents") {
            Self::parse_agents_config(agents_value)?
        } else {
            AgentsConfig::default()
        };

        let a2a_enabled = record
            .get("a2a_enabled")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false);

        // Extract optional 'session_store' config
        let session_store = if let Some(store_value) = record.get("session_store") {
            Some(Self::parse_session_store_config(store_value)?)
        } else {
            None
        };

        Ok(Self {
            models,
            providers,
            compaction,
            agents,
            a2a_enabled,
            session_store,
        })
    }

    /// Parse a single provider configuration
    fn parse_provider_config(
        value: &nu_protocol::Value,
    ) -> Result<ProviderConfig, nu_protocol::LabeledError> {
        use nu_protocol::LabeledError;

        fn parse_optional_preamble(
            record: &nu_protocol::Record,
            span: nu_protocol::Span,
        ) -> Result<Option<String>, LabeledError> {
            let Some(preamble_value) = record.get("preamble") else {
                return Ok(None);
            };

            let preamble = preamble_value
                .as_str()
                .map_err(|_| {
                    LabeledError::new("Invalid field type")
                        .with_label("'preamble' must be a string", span)
                })?
                .trim()
                .to_string();

            if preamble.is_empty() {
                Ok(None)
            } else {
                Ok(Some(preamble))
            }
        }

        let record = value.as_record().map_err(|_| {
            LabeledError::new("Invalid provider configuration")
                .with_label("Provider configuration must be a record", value.span())
        })?;

        // Extract optional fields
        let name = record
            .get("name")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

        let api_key = record
            .get("api_key")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

        let base_url = record
            .get("base_url")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

        let provider = record
            .get("provider")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

        let preamble = parse_optional_preamble(record, value.span())?;

        // Extract 'models' record (optional, defaults to empty)
        let models = if let Some(models_value) = record.get("models") {
            if let Ok(models_record) = models_value.as_record() {
                let mut models_map = HashMap::new();
                for (model_name, model_value) in models_record.iter() {
                    let model_config = Self::parse_model_config(model_value)?;
                    models_map.insert(model_name.clone(), model_config);
                }
                models_map
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        };

        Ok(ProviderConfig {
            name,
            api_key,
            base_url,
            provider,
            preamble,
            models,
        })
    }

    /// Parse a single model configuration
    fn parse_model_config(
        value: &nu_protocol::Value,
    ) -> Result<ModelConfig, nu_protocol::LabeledError> {
        use nu_protocol::LabeledError;

        fn parse_optional_preamble(
            record: &nu_protocol::Record,
            span: nu_protocol::Span,
        ) -> Result<Option<String>, LabeledError> {
            let Some(preamble_value) = record.get("preamble") else {
                return Ok(None);
            };

            let preamble = preamble_value
                .as_str()
                .map_err(|_| {
                    LabeledError::new("Invalid field type")
                        .with_label("'preamble' must be a string", span)
                })?
                .trim()
                .to_string();

            if preamble.is_empty() {
                Ok(None)
            } else {
                Ok(Some(preamble))
            }
        }

        let record = value.as_record().map_err(|_| {
            LabeledError::new("Invalid model configuration")
                .with_label("Model configuration must be a record", value.span())
        })?;

        // Extract optional 'name' field
        let name = record
            .get("name")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

        // Extract optional 'temperature' field
        let temperature = record.get("temperature").and_then(|v| v.as_float().ok());

        // Extract optional 'tool_call' field
        let tool_call = record.get("tool_call").and_then(|v| v.as_bool().ok());

        // Extract optional 'preamble' field with strict type handling
        let preamble = parse_optional_preamble(record, value.span())?;

        // Extract optional 'limit' field
        let limit = if let Some(limit_value) = record.get("limit") {
            if let Ok(limit_record) = limit_value.as_record() {
                Some(Self::parse_model_limits(limit_record)?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(ModelConfig {
            limit,
            name,
            temperature,
            preamble,
            tool_call,
        })
    }

    /// Parse model limits from record
    fn parse_model_limits(
        record: &nu_protocol::Record,
    ) -> Result<ModelLimits, nu_protocol::LabeledError> {
        // Helper to extract optional u32 field
        fn get_optional_u32(record: &nu_protocol::Record, key: &str) -> Option<u32> {
            record.get(key).and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
            })
        }

        let context = get_optional_u32(record, "context");
        let output = get_optional_u32(record, "output");

        Ok(ModelLimits { context, output })
    }

    /// Parse compaction configuration from a Nushell record
    fn parse_compaction_config(
        value: &nu_protocol::Value,
    ) -> Result<CompactionConfig, nu_protocol::LabeledError> {
        use nu_protocol::LabeledError;

        let record = value.as_record().map_err(|_| {
            LabeledError::new("Invalid compaction configuration")
                .with_label("Compaction configuration must be a record", value.span())
        })?;

        let span = value.span();

        // Parse optional 'strategy' field
        let strategy = if let Some(strategy_value) = record.get("strategy") {
            let s = strategy_value.as_str().map_err(|_| {
                LabeledError::new("Invalid field type")
                    .with_label("'strategy' must be a string", span)
            })?;
            let parsed: CompactionStrategy =
                serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(
                    |_| {
                        LabeledError::new("Invalid compaction strategy").with_label(
                            format!(
                            "Unknown strategy '{}'. Valid: sliding_summary, sliding_window, token_truncate",
                            s
                        ),
                            span,
                        )
                    },
                )?;
            Some(parsed)
        } else {
            None
        };

        // Helper to extract optional usize field
        fn get_optional_usize(record: &nu_protocol::Record, key: &str) -> Option<usize> {
            record.get(key).and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as usize) } else { None })
            })
        }

        let keep_recent = get_optional_usize(record, "keep_recent");
        let token_budget = get_optional_usize(record, "token_budget");

        // Parse optional 'proactive_threshold_pct' field
        let proactive_threshold_pct = record
            .get("proactive_threshold_pct")
            .and_then(|v| v.as_float().ok());

        Ok(CompactionConfig {
            strategy,
            keep_recent,
            token_budget,
            proactive_threshold_pct,
        })
    }

    /// Parse agents configuration from a Nushell record
    fn parse_agents_config(
        value: &nu_protocol::Value,
    ) -> Result<AgentsConfig, nu_protocol::LabeledError> {
        use nu_protocol::LabeledError;

        let record = value.as_record().map_err(|_| {
            LabeledError::new("Invalid agents config").with_label("expected a record", value.span())
        })?;

        let planner_enabled = record
            .get("planner")
            .and_then(|v| v.as_str().ok())
            .map(|s| s != "disabled")
            .unwrap_or(true);

        let maker_enabled = record
            .get("maker")
            .and_then(|v| v.as_str().ok())
            .map(|s| s != "disabled")
            .unwrap_or(true);

        let default = record
            .get("default")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "planner".to_string());

        let fallback = record
            .get("fallback")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

        Ok(AgentsConfig {
            planner_enabled,
            maker_enabled,
            default,
            fallback,
        })
    }

    /// Parse session store configuration from a Nushell record.
    ///
    /// Expected structure:
    /// ```nushell
    /// {
    ///   type: "sqlite" | "jsonl"
    ///   path: "/custom/path"  # optional
    /// }
    /// ```
    fn parse_session_store_config(
        value: &nu_protocol::Value,
    ) -> Result<StoreTypeConfig, nu_protocol::LabeledError> {
        use nu_protocol::LabeledError;

        let record = value.as_record().map_err(|_| {
            LabeledError::new("Invalid session_store configuration")
                .with_label("session_store must be a record", value.span())
        })?;

        let span = value.span();

        // Parse required 'type' field
        let type_str = record
            .get("type")
            .ok_or_else(|| {
                LabeledError::new("Missing required field")
                    .with_label("session_store must have a 'type' field", span)
            })?
            .as_str()
            .map_err(|_| {
                LabeledError::new("Invalid field type").with_label("'type' must be a string", span)
            })?;

        let store_type: StoreType = type_str.parse().map_err(|e: String| {
            LabeledError::new(format!("Invalid session_store type: {e}"))
                .with_label("expected 'sqlite' or 'jsonl'", span)
        })?;

        // Parse optional 'path' field
        let path = record
            .get("path")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

        Ok(StoreTypeConfig { store_type, path })
    }

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
    /// - Provider not found in configuration
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

        // Look up provider configuration
        let provider_config = self
            .providers
            .get(provider_name)
            .ok_or_else(|| format!("Provider '{}' not found in configuration", provider_name))?;

        // Look up model-specific configuration (optional)
        let model_config = provider_config.models.get(model_name);

        log::debug!(
            "resolve_model: spec={model_spec} provider={provider_name} model={model_name} config_found={}",
            model_config.is_some()
        );

        // Step 1: Start with env-based config for this provider/model (lowest priority)
        let mut config = Config::from_env(provider_name, model_name);

        // Step 2: Merge provider-level settings
        if let Some(impl_name) = &provider_config.provider {
            config.provider_impl = Some(impl_name.clone());
        }
        if let Some(api_key) = &provider_config.api_key {
            config.api_key = Some(api_key.clone());
        }
        if let Some(base_url) = &provider_config.base_url {
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

        // Forward global plugin config fields (not model-specific).
        // a2a_enabled is a bool, not Option<T> — this means "if env var didn't
        // set it to true, use the plugin config value". The edge case where an
        // env var is explicitly set to `false` (overridden by plugin's `true`)
        // is accepted for simplicity. Most users set this via plugin config.
        if !config.a2a_enabled {
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
    pub a2a_enabled: bool,

    /// A2A agent port (CLI-only). Some(0) = random, Some(n>0) = fixed, None = not set.
    pub a2a_port: Option<u16>,

    /// Session store backend type. None = use default (SQLite).
    pub session_store_type: Option<StoreType>,
}

/// Convert a `nu_protocol::Value` to a `serde_json::Value` without including
/// span metadata. `serde_json::to_value(&nu_value)` would include internal span
/// fields, so we convert manually.
fn nu_value_to_json(value: &nu_protocol::Value) -> Result<serde_json::Value, String> {
    match value {
        nu_protocol::Value::Int { val, .. } => Ok(serde_json::Value::Number((*val).into())),
        nu_protocol::Value::Float { val, .. } => serde_json::Number::from_f64(*val)
            .map(serde_json::Value::Number)
            .ok_or_else(|| format!("non-finite float: {val}")),
        nu_protocol::Value::String { val, .. } => Ok(serde_json::Value::String(val.clone())),
        nu_protocol::Value::Bool { val, .. } => Ok(serde_json::Value::Bool(*val)),
        nu_protocol::Value::Nothing { .. } => Ok(serde_json::Value::Null),
        nu_protocol::Value::List { vals, .. } => {
            let arr = vals
                .iter()
                .map(nu_value_to_json)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(serde_json::Value::Array(arr))
        }
        nu_protocol::Value::Record { val, .. } => {
            let obj = val
                .iter()
                .map(|(k, v)| nu_value_to_json(v).map(|j| (k.clone(), j)))
                .collect::<Result<serde_json::Map<_, _>, _>>()?;
            Ok(serde_json::Value::Object(obj))
        }
        other => Err(format!("unsupported nu value type: {:?}", other.get_type())),
    }
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
        let a2a_enabled: bool = env::var("AGENT_A2A_ENABLED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(false);

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

    /// Merge this config with another, with the other taking precedence.
    ///
    /// For each field:
    /// - Required fields (provider, model): always take from `other`
    /// - Optional fields: use `other`'s value if Some, otherwise keep `self`'s value
    ///
    /// This allows layering configs: base.merge(override).merge(cli_args)
    pub fn merge(self, other: Self) -> Self {
        log::debug!(
            "Config.merge: provider={} model={}",
            other.provider,
            other.model
        );
        Self {
            // Required fields always from other
            provider: other.provider,
            provider_impl: other.provider_impl.or(self.provider_impl),
            model: other.model,

            // Optional fields: other if Some, else self
            api_key: other.api_key.or(self.api_key),
            base_url: other.base_url.or(self.base_url),
            temperature: other.temperature.or(self.temperature),
            max_tokens: other.max_tokens.or(self.max_tokens),
            max_context_tokens: other.max_context_tokens.or(self.max_context_tokens),
            max_output_tokens: other.max_output_tokens.or(self.max_output_tokens),
            max_tool_turns: other.max_tool_turns.or(self.max_tool_turns),
            preamble: other.preamble.or(self.preamble),
            read_timeout_secs: other.read_timeout_secs.or(self.read_timeout_secs),
            max_tool_result_bytes: other.max_tool_result_bytes.or(self.max_tool_result_bytes),
            model_context_tokens: other.model_context_tokens.or(self.model_context_tokens),
            context_warning_threshold: other
                .context_warning_threshold
                .or(self.context_warning_threshold),
            max_retries: other.max_retries.or(self.max_retries),
            retry_base_delay_ms: other.retry_base_delay_ms.or(self.retry_base_delay_ms),
            max_tool_calls_per_subturn: other
                .max_tool_calls_per_subturn
                .or(self.max_tool_calls_per_subturn),
            additional_params: other.additional_params.or(self.additional_params),
            a2a_enabled: other.a2a_enabled || self.a2a_enabled,
            a2a_port: other.a2a_port.or(self.a2a_port),
            session_store_type: other.session_store_type.or(self.session_store_type),
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
