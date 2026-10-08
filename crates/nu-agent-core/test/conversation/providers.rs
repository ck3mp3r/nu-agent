use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn build_copilot_client_function_signature_exists() {
    // Compile-time verification that build_copilot_client exists with correct signature
    use crate::config::Config;
    use nu_protocol::LabeledError;

    // Type annotation forces the compiler to verify the function signature
    let _function: fn(
        &Config,
    ) -> std::result::Result<rig::providers::copilot::Copilot, LabeledError> = build_copilot_client;

    // If this compiles, the function exists with the correct signature
}

#[test]
fn build_copilot_client_no_auth_returns_ok() -> Result<()> {
    // With no credential resolved, the client is still constructible — rig 0.43
    // resolves OAuth lazily through `Authenticator` (task 6793a293), so auth
    // failures surface at request time, not at build time.
    use crate::config::Config;
    use crate::utils::env_map::EnvMap;

    // Empty env map and a temp token dir keep the test off the real environment
    // and away from any cached OAuth tokens.
    let token_dir = tempfile::TempDir::new()?;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let result = build_copilot_client_with(&config, &EnvMap::new(), Some(token_dir.path()));

    assert!(
        result.is_ok(),
        "OAuth path always succeeds at build time; auth is lazy. Got: {:?}",
        result.err()
    );
    Ok(())
}

#[test]
fn build_copilot_client_oauth_path_succeeds_at_build_time() -> Result<()> {
    // With no credential resolved, the client is still constructible — rig 0.43
    // resolves OAuth lazily through `Authenticator` (task 6793a293), so auth
    // failures surface at request time, not at build time.
    use crate::config::Config;
    use crate::utils::env_map::EnvMap;

    let token_dir = tempfile::TempDir::new()?;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "gpt-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        output_budget_empty_remedy: None,
        output_budget_remedy_mode: None,
        output_budget_raise_enabled: None,
        output_budget_raise_multiplier: None,
        output_budget_raise_cap: None,
        repetition_guard: None,
        max_tool_calls_per_subturn: None,
        additional_params: None,
        a2a_enabled: None,
        session_store_type: None,
    };

    let result = build_copilot_client_with(&config, &EnvMap::new(), Some(token_dir.path()));

    assert!(
        result.is_ok(),
        "OAuth path always succeeds at build time; auth is lazy. Got: {:?}",
        result.err()
    );
    Ok(())
}

#[test]
fn resolve_provider_type_uses_explicit_field() {
    assert_eq!(
        super::resolve_provider_type("ollama-remote", Some("ollama")),
        "ollama"
    );
}

#[test]
fn resolve_provider_type_falls_back_to_key() {
    assert_eq!(super::resolve_provider_type("ollama", None), "ollama");
}

#[test]
fn resolve_provider_type_custom_key_with_known_impl() {
    assert_eq!(
        super::resolve_provider_type("my-openai", Some("openai")),
        "openai"
    );
}

// ========================================================================
// HTTP client timeout tests
// ========================================================================

#[test]
fn build_http_client_returns_configured_client()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let client = super::build_http_client(None)?;
    drop(client);
    Ok(())
}

#[test]
fn build_ollama_client_with_base_url_succeeds() {
    use crate::config::Config;

    let config = Config {
        api_key: None,
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    };
    let result = super::build_ollama_client(&config);
    assert!(result.is_ok());
}

#[test]
fn build_ollama_client_with_api_key_succeeds() {
    use crate::config::Config;

    let config = Config {
        api_key: Some("test-api-key".to_string()),
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    };
    let result = super::build_ollama_client(&config);
    assert!(result.is_ok());
}

// ========================================================================
// rig 0.43 client construction: base_url and route propagation
// ========================================================================

#[test]
fn build_openai_client_sets_base_url_on_config() -> Result<()> {
    use crate::config::Config;

    // -- Setup & Fixtures
    let config = Config {
        api_key: Some("sk-fake".to_string()),
        base_url: Some("http://localhost:8080/v1".to_string()),
        ..Config::default()
    };

    // -- Exec
    let client = build_openai_client(&config).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(client.config().base_url, "http://localhost:8080/v1");
    Ok(())
}

#[test]
fn build_openai_client_with_base_url_uses_chat_route() -> Result<()> {
    use crate::config::Config;
    use rig::providers::openai::Route;

    // -- Setup & Fixtures
    let config = Config {
        api_key: Some("sk-fake".to_string()),
        base_url: Some("http://localhost:8080/v1".to_string()),
        ..Config::default()
    };

    // -- Exec
    let client = build_openai_client(&config).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(client.config().completion_route(), Route::Chat);
    Ok(())
}

#[test]
fn build_openai_client_without_base_url_uses_responses_route() -> Result<()> {
    use crate::config::Config;
    use rig::providers::openai::Route;

    // -- Setup & Fixtures
    let config = Config {
        api_key: Some("sk-fake".to_string()),
        base_url: None,
        ..Config::default()
    };

    // -- Exec
    let client = build_openai_client(&config).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(client.config().completion_route(), Route::Responses);
    Ok(())
}

#[test]
fn build_anthropic_client_sets_base_url_on_config() -> Result<()> {
    use crate::config::Config;

    // -- Setup & Fixtures
    let config = Config {
        api_key: Some("sk-ant-fake".to_string()),
        base_url: Some("http://localhost:8080".to_string()),
        ..Config::default()
    };

    // -- Exec
    let client = build_anthropic_client(&config).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(client.config().base_url, "http://localhost:8080");
    Ok(())
}

#[test]
fn build_ollama_client_sets_base_url_on_config() -> Result<()> {
    use crate::config::Config;

    // -- Setup & Fixtures
    let config = Config {
        api_key: None,
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    };

    // -- Exec
    let client = build_ollama_client(&config).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(client.config().base_url, "http://localhost:11434");
    Ok(())
}

#[test]
fn build_ollama_client_without_api_key_leaves_secret_empty() -> Result<()> {
    use crate::config::Config;

    // -- Setup & Fixtures
    let config = Config {
        api_key: None,
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    };

    // -- Exec
    let client = build_ollama_client(&config).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert!(client.config().api_key.is_empty());
    Ok(())
}

#[test]
fn build_ollama_client_with_api_key_sets_secret() -> Result<()> {
    use crate::config::Config;

    // -- Setup & Fixtures
    let config = Config {
        api_key: Some("test-api-key".to_string()),
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    };

    // -- Exec
    let client = build_ollama_client(&config).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(client.config().api_key.expose(), "test-api-key");
    Ok(())
}

#[test]
fn build_copilot_client_sets_base_url_on_config() -> Result<()> {
    use crate::config::Config;

    // -- Setup & Fixtures
    let config = Config {
        api_key: Some("fake-token".to_string()),
        base_url: Some("http://localhost:9999".to_string()),
        ..Config::default()
    };

    // -- Exec
    let client = build_copilot_client(&config).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(client.config().base_url, "http://localhost:9999");
    Ok(())
}

// ========================================================================
// Phase 1b: CachedProviderClient, resolve_provider_type, ClientCacheKey
// ========================================================================

#[test]
fn client_cache_key_type_is_four_tuple() {
    let key: ClientCacheKey = ("copilot".to_string(), None, None, None);
    assert_eq!(key.0, "copilot");
    assert_eq!(key.1, None);
    assert_eq!(key.2, None);
    assert_eq!(key.3, None);
}

#[test]
fn resolve_provider_type_uses_field_when_set() {
    assert_eq!(
        super::resolve_provider_type("my-provider", Some("copilot")),
        "copilot"
    );
}

#[test]
fn resolve_provider_type_falls_back_to_key_when_field_none() {
    assert_eq!(
        super::resolve_provider_type("github-copilot", None),
        "github-copilot"
    );
}

#[test]
fn cached_provider_client_copilot_variant_holds_client() -> Result<()> {
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "copilot".to_string(),
        model: "gpt-4".to_string(),
        api_key: Some("fake-token".to_string()),
        ..Config::default()
    };
    let client = build_copilot_client(&config).map_err(|e| format!("{e:?}"))?;
    let c = CachedProviderClient::Copilot(client);
    assert!(matches!(c, CachedProviderClient::Copilot(_)));
    Ok(())
}

#[test]
fn cached_provider_client_openai_variant_holds_client() -> Result<()> {
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        api_key: Some("sk-fake".to_string()),
        ..Config::default()
    };
    let client = build_openai_client(&config).map_err(|e| format!("{e:?}"))?;
    let c = CachedProviderClient::OpenAi(client);
    assert!(matches!(c, CachedProviderClient::OpenAi(_)));
    Ok(())
}

#[test]
fn cached_provider_client_ollama_variant_holds_client() -> Result<()> {
    use crate::config::Config;

    let config = Config {
        a2a_port: None,
        provider: "ollama".to_string(),
        model: "llama3".to_string(),
        api_key: None,
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    };
    let client = build_ollama_client(&config).map_err(|e| format!("{e:?}"))?;
    let c = CachedProviderClient::Ollama(client);
    assert!(matches!(c, CachedProviderClient::Ollama(_)));
    Ok(())
}

// ========================================================================
// OpenAI variant selection: base_url vs no base_url
// ========================================================================

#[test]
fn openai_without_base_url_produces_openai_variant() -> Result<()> {
    use crate::config::Config;
    use crate::conversation::state::provider::ProviderState;

    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        api_key: Some("sk-fake".to_string()),
        base_url: None,
        ..Config::default()
    };
    let mut state = ProviderState::new(config, None);
    state.ensure_client_cached().map_err(|e| format!("{e:?}"))?;
    let client = state
        .client()
        .ok_or("client should be cached after ensure_client_cached")?;
    assert!(matches!(client, CachedProviderClient::OpenAi(_)));
    Ok(())
}

#[test]
fn openai_with_base_url_produces_openai_completions_variant() -> Result<()> {
    use crate::config::Config;
    use crate::conversation::state::provider::ProviderState;

    let config = Config {
        a2a_port: None,
        provider: "openai".to_string(),
        model: "mistral-7b".to_string(),
        api_key: Some("sk-fake".to_string()),
        base_url: Some("http://localhost:8080/v1".to_string()),
        ..Config::default()
    };
    let mut state = ProviderState::new(config, None);
    state.ensure_client_cached().map_err(|e| format!("{e:?}"))?;
    let client = state
        .client()
        .ok_or("client should be cached after ensure_client_cached")?;
    assert!(matches!(client, CachedProviderClient::OpenAiCompletions(_)));
    Ok(())
}

// ========================================================================
// read_timeout_secs pass-through tests
// ========================================================================

#[test]
fn plugin_config_read_timeout_secs_propagates_to_resolved_config() -> Result<()> {
    use std::collections::HashMap;

    use crate::config::{AgentsConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: None,
            api_key: Some("sk-test".to_string()),
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );

    let plugin_config = PluginConfig {
        models: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    read_timeout_secs: Some(60),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        read_timeout_secs: Some(60),
        ..ModelRoleConfig::default()
    };
    let resolved = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(
        resolved.read_timeout_secs,
        Some(60),
        "read_timeout_secs should be 60 after resolve"
    );
    Ok(())
}

#[test]
fn plugin_config_without_read_timeout_secs_resolves_to_none() -> Result<()> {
    use std::collections::HashMap;

    use crate::config::{AgentsConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: None,
            api_key: Some("sk-test".to_string()),
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );

    let plugin_config = PluginConfig {
        models: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };
    let resolved = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(
        resolved.read_timeout_secs, None,
        "read_timeout_secs should be None when not configured"
    );
    Ok(())
}
