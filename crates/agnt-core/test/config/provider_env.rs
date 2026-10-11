//! Provider-specific `Config::from_env()` API-key behavior tests.

use crate::config::Config;
use crate::test_support::env_map;

#[test]
fn copilot_provider_does_not_set_api_key_from_env() {
    // Test that copilot provider does NOT populate api_key from GITHUB_COPILOT_API_KEY
    // rig's from_env() handles environment variable resolution internally
    let env = env_map(&[("GITHUB_COPILOT_API_KEY", "test_key")]);
    let config = Config::from_env(&env, "copilot", "claude");

    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert_eq!(config.api_key, None);
}

#[test]
fn github_copilot_provider_does_not_set_api_key_from_env() {
    // Test that github-copilot provider does NOT populate api_key from GITHUB_COPILOT_API_KEY
    // rig's from_env() handles environment variable resolution internally
    let env = env_map(&[("GITHUB_COPILOT_API_KEY", "test_key")]);
    let config = Config::from_env(&env, "github-copilot", "gpt-4o");

    assert_eq!(config.provider, "github-copilot");
    assert_eq!(config.model, "gpt-4o");
    assert_eq!(config.api_key, None);
}

#[test]
fn non_copilot_provider_sets_api_key_from_env() {
    // Test that non-copilot providers (e.g., openai) DO populate api_key from env vars
    let env = env_map(&[("OPENAI_API_KEY", "sk-test123")]);
    let config = Config::from_env(&env, "openai", "gpt-4");

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
}

#[test]
fn additional_params_defaults_to_none() {
    let config = Config::default();
    assert!(config.additional_params.is_none());
}
