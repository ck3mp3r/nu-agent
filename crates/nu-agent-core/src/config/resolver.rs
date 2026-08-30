use super::{Config, ModelRoleConfig, PluginConfig};

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
            format!("Invalid model specification '{model_spec}'. Expected 'provider/model' format")
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
        if let Some(store) = &self.secret_store
            && let Some(key) = &config.api_key
            && let Some(resolved) = store.resolve(key)
        {
            config.api_key = Some(resolved);
        }

        // Apply models.json cache specs (fill missing values only)
        if let Some(cache) = &self.models_cache
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
