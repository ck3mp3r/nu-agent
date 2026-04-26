use std::collections::HashMap;

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
    pub provider_impl: Option<String>,

    /// Models available for this provider
    pub models: HashMap<String, ModelConfig>,
}

/// Top-level plugin configuration (provider-centric)
#[derive(Debug, Clone, PartialEq)]
pub struct PluginConfig {
    /// Active model (provider/model format)
    pub model: String,

    /// Small/fast model for simple tasks (optional)
    pub small_model: Option<String>,

    /// Provider configurations
    pub providers: HashMap<String, ProviderConfig>,
}

impl PluginConfig {
    /// Parse PluginConfig from Nushell record
    ///
    /// Expected structure:
    /// ```nushell
    /// {
    ///   model: "openai/gpt-4"
    ///   small_model: "openai/gpt-3.5-turbo"  # optional
    ///   providers: {
    ///     openai: {
    ///       name: "OpenAI"  # optional
    ///       api_key: "sk-..."  # optional
    ///       base_url: "https://..."  # optional
    ///       provider_impl: "openai"  # optional, for custom providers
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

        // Extract required 'model' field
        let model = record
            .get("model")
            .ok_or_else(|| labeled_error("Missing required field", "Missing 'model' field", span))?
            .as_str()
            .map_err(|_| labeled_error("Invalid field type", "'model' must be a string", span))?
            .to_string();

        // Extract optional 'small_model' field
        let small_model = record
            .get("small_model")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

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

        Ok(Self {
            model,
            small_model,
            providers,
        })
    }

    /// Parse a single provider configuration
    fn parse_provider_config(
        value: &nu_protocol::Value,
    ) -> Result<ProviderConfig, nu_protocol::LabeledError> {
        use nu_protocol::LabeledError;

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

        let provider_impl = record
            .get("provider_impl")
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string());

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
            provider_impl,
            models,
        })
    }

    /// Parse a single model configuration
    fn parse_model_config(
        value: &nu_protocol::Value,
    ) -> Result<ModelConfig, nu_protocol::LabeledError> {
        use nu_protocol::LabeledError;

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

    /// Resolve a model specification (provider/model format) to a Config
    ///
    /// Takes a model specification like "openai/gpt-4" and:
    /// 1. Parses provider and model name
    /// 2. Looks up provider configuration
    /// 3. Looks up model-specific configuration (if exists)
    /// 4. Merges provider and model config with env vars
    /// 5. Returns runtime Config
    ///
    /// # Arguments
    /// * `model_spec` - Model specification in "provider/model" format
    ///
    /// # Returns
    /// * `Ok(Config)` - Runtime configuration
    /// * `Err(String)` - Error message if parsing or lookup fails
    pub fn resolve_model(&self, model_spec: &str) -> Result<Config, String> {
        // Parse provider/model format - support both 2-part and 3-part formats
        let parts: Vec<&str> = model_spec.split('/').collect();

        let (provider_name, model_name) = match parts.len() {
            2 => {
                // Traditional format: "openai/gpt-4"
                (parts[0].to_string(), parts[1])
            }
            3 if parts[0] == "github-copilot" => {
                // New format: "github-copilot/anthropic/claude-sonnet-4.5"
                // Validate backend (parts[1]) is non-empty before constructing provider
                if parts[1].is_empty() {
                    return Err("Backend name cannot be empty".to_string());
                }
                // Provider becomes "github-copilot/anthropic"
                (format!("{}/{}", parts[0], parts[1]), parts[2])
            }
            3 => {
                return Err(format!(
                    "3-part format only allowed for github-copilot, got: {}",
                    model_spec
                ));
            }
            _ => {
                return Err(format!(
                    "Invalid model specification '{}'. Expected 'provider/model' or 'github-copilot/backend/model'",
                    model_spec
                ));
            }
        };

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
            .get(&provider_name)
            .ok_or_else(|| format!("Provider '{}' not found in configuration", provider_name))?;

        // Look up model-specific configuration (optional)
        let model_config = provider_config.models.get(model_name);

        // Start with env-based config for this provider/model
        // Use the actual provider name, not provider_impl
        let mut config = Config::from_env(&provider_name, model_name);

        // Set provider_impl if specified in provider config
        if let Some(impl_name) = &provider_config.provider_impl {
            config.provider_impl = Some(impl_name.clone());
        }

        // Merge provider-level settings
        if let Some(api_key) = &provider_config.api_key {
            config.api_key = Some(api_key.clone());
        }
        if let Some(base_url) = &provider_config.base_url {
            config.base_url = Some(base_url.clone());
        }

        // Merge model-specific settings (if model exists in config)
        if let Some(model_cfg) = model_config {
            if let Some(temp) = model_cfg.temperature {
                config.temperature = Some(temp);
            }

            // Merge limits
            if let Some(limits) = &model_cfg.limit {
                if let Some(context) = limits.context {
                    config.max_context_tokens = Some(context);
                }
                if let Some(output) = limits.output {
                    config.max_output_tokens = Some(output);
                }
            }
        }

        Ok(config)
    }
}

/// Runtime configuration for a specific invocation
#[derive(Debug, Clone, PartialEq)]
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
}

impl Config {
    /// Create a Config by reading environment variables.
    ///
    /// Looks for:
    /// - `{PROVIDER}_API_KEY` (e.g., `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`)
    /// - `AGENT_TEMPERATURE`, `AGENT_MAX_TOKENS`, etc. for overrides
    ///
    /// Invalid values are gracefully ignored (set to None).
    pub fn from_env(provider: &str, model: &str) -> Self {
        use std::env;

        // Helper to parse environment variable with error handling
        fn parse_env_var<T: std::str::FromStr>(key: &str) -> Option<T> {
            env::var(key).ok().and_then(|val| val.parse().ok())
        }

        // Provider-specific API key (e.g., OPENAI_API_KEY)
        let provider_upper = provider.to_uppercase();
        let api_key_var = format!("{}_API_KEY", provider_upper);
        let api_key = env::var(&api_key_var).ok();

        // AGENT_* overrides
        let base_url = env::var("AGENT_BASE_URL").ok();
        let temperature = parse_env_var("AGENT_TEMPERATURE");
        let max_tokens = parse_env_var("AGENT_MAX_TOKENS");
        let max_context_tokens = parse_env_var("AGENT_MAX_CONTEXT_TOKENS");
        let max_output_tokens = parse_env_var("AGENT_MAX_OUTPUT_TOKENS");
        let max_tool_turns = parse_env_var("AGENT_MAX_TOOL_TURNS").or(Some(20)); // Default to 20 if not overridden

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
        }
    }

    /// Create a Config from Nushell plugin configuration.
    ///
    /// Expects a Record value with fields:
    /// - Required: provider (String), model (String)
    /// - Optional: api_key, base_url, temperature, max_tokens,
    ///   max_context_tokens, max_output_tokens, max_tool_turns
    ///
    /// Returns an error if:
    /// - Value is not a Record
    /// - Required fields are missing
    ///
    /// Invalid types for optional fields are gracefully ignored (set to None).
    pub fn from_plugin_config(
        value: &nu_protocol::Value,
    ) -> Result<Self, nu_protocol::LabeledError> {
        // Ensure value is a record
        let record = value.as_record().map_err(|_| {
            nu_protocol::LabeledError::new("Invalid config")
                .with_label("Expected a record for plugin configuration", value.span())
        })?;

        // Helper to extract required string field
        fn get_required_string(
            record: &nu_protocol::Record,
            key: &str,
            span: nu_protocol::Span,
        ) -> Result<String, nu_protocol::LabeledError> {
            record
                .get(key)
                .ok_or_else(|| {
                    nu_protocol::LabeledError::new("Missing required field")
                        .with_label(format!("Missing '{}' field", key), span)
                })?
                .as_str()
                .map(|s| s.to_string())
                .map_err(|_| {
                    nu_protocol::LabeledError::new("Invalid field type")
                        .with_label(format!("'{}' must be a string", key), span)
                })
        }

        // Helper to extract optional string field
        fn get_optional_string(record: &nu_protocol::Record, key: &str) -> Option<String> {
            record
                .get(key)
                .and_then(|v| v.as_str().ok())
                .map(|s| s.to_string())
        }

        // Helper to extract optional float field
        fn get_optional_float(record: &nu_protocol::Record, key: &str) -> Option<f64> {
            record.get(key).and_then(|v| v.as_float().ok())
        }

        // Helper to extract optional int field as u32
        fn get_optional_u32(record: &nu_protocol::Record, key: &str) -> Option<u32> {
            record.get(key).and_then(|v| {
                v.as_int()
                    .ok()
                    .and_then(|i| if i >= 0 { Some(i as u32) } else { None })
            })
        }

        let span = value.span();

        // Extract required fields
        let provider = get_required_string(record, "provider", span)?;
        let model = get_required_string(record, "model", span)?;

        // Extract optional fields
        let api_key = get_optional_string(record, "api_key");
        let base_url = get_optional_string(record, "base_url");
        let temperature = get_optional_float(record, "temperature");
        let max_tokens = get_optional_u32(record, "max_tokens");
        let max_context_tokens = get_optional_u32(record, "max_context_tokens");
        let max_output_tokens = get_optional_u32(record, "max_output_tokens");
        let max_tool_turns = get_optional_u32(record, "max_tool_turns").or(Some(20)); // Default to 20 if not provided

        Ok(Self {
            provider,
            provider_impl: None, // from_plugin_config doesn't use provider_impl
            model,
            api_key,
            base_url,
            temperature,
            max_tokens,
            max_context_tokens,
            max_output_tokens,
            max_tool_turns,
        })
    }

    /// Merge this config with another, with the other taking precedence.
    ///
    /// For each field:
    /// - Required fields (provider, model): always take from `other`
    /// - Optional fields: use `other`'s value if Some, otherwise keep `self`'s value
    ///
    /// This allows layering configs: base.merge(override).merge(cli_args)
    pub fn merge(self, other: Self) -> Self {
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

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: String::new(),
            provider_impl: None,
            model: String::new(),
            api_key: None,
            base_url: None,
            temperature: None,
            max_tokens: None,
            max_context_tokens: None,
            max_output_tokens: None,
            max_tool_turns: Some(20),
        }
    }
}

#[cfg(test)]
mod tests;
