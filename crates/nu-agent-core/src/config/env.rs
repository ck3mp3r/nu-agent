use crate::session::StoreType;

use super::Config;

impl Config {
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
            let api_key_var = format!("{provider_upper}_API_KEY");
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
        let output_budget_empty_remedy = env::var("AGENT_OUTPUT_BUDGET_EMPTY_REMEDY").ok();
        let output_budget_remedy_mode = env::var("AGENT_OUTPUT_BUDGET_REMEDY_MODE").ok();
        let output_budget_raise_enabled: Option<bool> =
            parse_env_var("AGENT_OUTPUT_BUDGET_RAISE_ENABLED");
        let output_budget_raise_multiplier: Option<f64> =
            parse_env_var("AGENT_OUTPUT_BUDGET_RAISE_MULTIPLIER");
        let output_budget_raise_cap: Option<u32> = parse_env_var("AGENT_OUTPUT_BUDGET_RAISE_CAP");
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
            output_budget_empty_remedy,
            output_budget_remedy_mode,
            output_budget_raise_enabled,
            output_budget_raise_multiplier,
            output_budget_raise_cap,
            max_tool_calls_per_subturn,
            additional_params: None,
            a2a_enabled,
            a2a_port,
            session_store_type,
        }
    }
}
