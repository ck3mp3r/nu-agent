use super::*;

#[test]
fn build_copilot_client_function_signature_exists() {
    // Compile-time verification that build_copilot_client exists with correct signature
    use crate::config::Config;
    use nu_protocol::LabeledError;

    // Type annotation forces the compiler to verify the function signature
    let _function: fn(&Config) -> Result<rig::providers::copilot::Client, LabeledError> =
        build_copilot_client;

    // If this compiles, the function exists with the correct signature
}

#[test]
#[serial_test::serial]
fn build_copilot_client_no_auth_returns_error() {
    // RED: Verify that with no auth available, we get a clear error
    use crate::config::Config;

    // Save original XDG_CONFIG_HOME if set
    let original_xdg = std::env::var("XDG_CONFIG_HOME").ok();

    // Clear all copilot-related env vars to ensure clean test
    unsafe {
        std::env::remove_var("GITHUB_COPILOT_API_KEY");
        std::env::remove_var("COPILOT_API_KEY");
        std::env::remove_var("COPILOT_GITHUB_ACCESS_TOKEN");
        std::env::remove_var("GITHUB_TOKEN");
        std::env::remove_var("GITHUB_COPILOT_API_BASE");
        std::env::remove_var("COPILOT_BASE_URL");
        // Point XDG_CONFIG_HOME to non-existent directory to avoid cached tokens
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/nonexistent_test_dir_12345");
    }

    let config = Config {
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
    };

    let result = build_copilot_client(&config);

    // Restore original XDG_CONFIG_HOME
    unsafe {
        if let Some(val) = original_xdg {
            std::env::set_var("XDG_CONFIG_HOME", val);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    assert!(result.is_err(), "Expected error without credentials");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Not authenticated"),
        "Error should mention 'Not authenticated', got: {err_msg}"
    );
}

#[test]
#[serial_test::serial]
fn build_copilot_client_error_mentions_auth_login() {
    // RED: Verify error message guides user to run `agent auth login`
    use crate::config::Config;

    // Save original XDG_CONFIG_HOME if set
    let original_xdg = std::env::var("XDG_CONFIG_HOME").ok();

    // Clear all copilot-related env vars
    unsafe {
        std::env::remove_var("GITHUB_COPILOT_API_KEY");
        std::env::remove_var("COPILOT_API_KEY");
        std::env::remove_var("COPILOT_GITHUB_ACCESS_TOKEN");
        std::env::remove_var("GITHUB_TOKEN");
        std::env::remove_var("GITHUB_COPILOT_API_BASE");
        std::env::remove_var("COPILOT_BASE_URL");
        // Point XDG_CONFIG_HOME to non-existent directory to avoid cached tokens
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/nonexistent_test_dir_12345");
    }

    let config = Config {
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
    };

    let result = build_copilot_client(&config);

    // Restore original XDG_CONFIG_HOME
    unsafe {
        if let Some(val) = original_xdg {
            std::env::set_var("XDG_CONFIG_HOME", val);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    assert!(result.is_err(), "Expected error without credentials");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("agent auth login"),
        "Error should mention 'agent auth login', got: {err_msg}"
    );
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
fn build_http_client_returns_configured_client() {
    // Install crypto provider needed by reqwest+rustls
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = super::build_http_client(None);
    drop(client);
}

#[test]
fn build_ollama_client_with_base_url_succeeds() {
    use crate::config::Config;

    // Install crypto provider needed by reqwest+rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = Config {
        api_key: None,
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    };
    let result = super::build_ollama_client(&config);
    assert!(result.is_ok());
}

// ========================================================================
// Phase 1b: CachedProviderClient, resolve_provider_type, ClientCacheKey
// ========================================================================

#[test]
fn client_cache_key_type_is_three_tuple() {
    let key: ClientCacheKey = ("copilot".to_string(), None, None);
    assert_eq!(key.0, "copilot");
    assert_eq!(key.1, None);
    assert_eq!(key.2, None);
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
#[serial_test::serial]
fn cached_provider_client_copilot_variant_holds_client() {
    use crate::config::Config;

    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = Config {
        provider: "copilot".to_string(),
        model: "gpt-4".to_string(),
        api_key: Some("fake-token".to_string()),
        ..Config::default()
    };
    let client = build_copilot_client(&config).unwrap();
    let c = CachedProviderClient::Copilot(client);
    assert!(matches!(c, CachedProviderClient::Copilot(_)));
}

#[test]
#[serial_test::serial]
fn cached_provider_client_openai_variant_holds_client() {
    use crate::config::Config;

    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = Config {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        api_key: Some("sk-fake".to_string()),
        ..Config::default()
    };
    let client = build_openai_client(&config).unwrap();
    let c = CachedProviderClient::OpenAi(client);
    assert!(matches!(c, CachedProviderClient::OpenAi(_)));
}

#[test]
#[serial_test::serial]
fn cached_provider_client_ollama_variant_holds_client() {
    use crate::config::Config;

    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = Config {
        provider: "ollama".to_string(),
        model: "llama3".to_string(),
        api_key: None,
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    };
    let client = build_ollama_client(&config).unwrap();
    let c = CachedProviderClient::Ollama(client);
    assert!(matches!(c, CachedProviderClient::Ollama(_)));
}

// ========================================================================
// OpenAI variant selection: base_url vs no base_url
// ========================================================================

#[test]
#[serial_test::serial]
fn openai_without_base_url_produces_openai_variant() {
    use crate::config::Config;
    use crate::conversation::state::provider::ProviderState;

    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        api_key: Some("sk-fake".to_string()),
        base_url: None,
        ..Config::default()
    };
    let mut state = ProviderState::new(config, None);
    state.ensure_client_cached().unwrap();
    assert!(matches!(
        state.client().unwrap(),
        CachedProviderClient::OpenAi(_)
    ));
}

#[test]
#[serial_test::serial]
fn openai_with_base_url_produces_openai_completions_variant() {
    use crate::config::Config;
    use crate::conversation::state::provider::ProviderState;

    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = Config {
        provider: "openai".to_string(),
        model: "mistral-7b".to_string(),
        api_key: Some("sk-fake".to_string()),
        base_url: Some("http://localhost:8080/v1".to_string()),
        ..Config::default()
    };
    let mut state = ProviderState::new(config, None);
    state.ensure_client_cached().unwrap();
    assert!(matches!(
        state.client().unwrap(),
        CachedProviderClient::OpenAiCompletions(_)
    ));
}

// ========================================================================
// read_timeout_secs pass-through tests
// ========================================================================

#[test]
fn plugin_config_read_timeout_secs_propagates_to_resolved_config() {
    use std::collections::HashMap;

    use crate::config::{AgentsConfig, PluginConfig, ProviderConfig};

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
        model: "openai/gpt-4".to_string(),
        small_model: None,
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        read_timeout_secs: Some(60),
    };

    let resolved = plugin_config
        .resolve_model("openai/gpt-4")
        .expect("should resolve");

    assert_eq!(
        resolved.read_timeout_secs,
        Some(60),
        "read_timeout_secs should be 60 after resolve"
    );
}

#[test]
fn plugin_config_without_read_timeout_secs_resolves_to_none() {
    use std::collections::HashMap;

    use crate::config::{AgentsConfig, PluginConfig, ProviderConfig};

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
        model: "openai/gpt-4".to_string(),
        small_model: None,
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        read_timeout_secs: None,
    };

    let resolved = plugin_config
        .resolve_model("openai/gpt-4")
        .expect("should resolve");

    assert_eq!(
        resolved.read_timeout_secs, None,
        "read_timeout_secs should be None when not configured"
    );
}
