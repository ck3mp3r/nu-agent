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
    let client = super::build_http_client();
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
