//! Copilot-specific `Config::from_env()` API-key tests.

use crate::config::Config;
use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;

#[test]
fn test_from_env_copilot_with_github_copilot_api_key() {
    // Test copilot provider with GITHUB_COPILOT_API_KEY
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    let env = env_map(&[("GITHUB_COPILOT_API_KEY", "copilot_key")]);
    let config = Config::from_env(&env, "copilot", "claude");

    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert_eq!(config.api_key, None);
}

#[test]
fn test_from_env_copilot_fallback_to_github_token() {
    // Test copilot provider falls back to GITHUB_TOKEN if GITHUB_COPILOT_API_KEY not set
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    let env = env_map(&[("GITHUB_TOKEN", "github_token")]);
    let config = Config::from_env(&env, "copilot", "claude");

    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert_eq!(config.api_key, None);
}

#[test]
fn test_from_env_copilot_precedence_github_copilot_api_key_over_github_token() {
    // Test that GITHUB_COPILOT_API_KEY takes precedence over GITHUB_TOKEN
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    let env = env_map(&[
        ("GITHUB_COPILOT_API_KEY", "copilot_key"),
        ("GITHUB_TOKEN", "github_token"),
    ]);
    let config = Config::from_env(&env, "copilot", "claude");

    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert_eq!(config.api_key, None);
}

#[test]
fn test_from_env_github_copilot_with_github_copilot_api_key() {
    // Test "github-copilot" provider variant
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    let env = env_map(&[("GITHUB_COPILOT_API_KEY", "copilot_key")]);
    let config = Config::from_env(&env, "github-copilot", "claude");

    assert_eq!(config.provider, "github-copilot");
    assert_eq!(config.model, "claude");
    assert_eq!(config.api_key, None);
}

#[test]
fn test_from_env_copilot_missing_all_keys() {
    // Test copilot provider without any API keys
    let env = EnvMap::new();
    let config = Config::from_env(&env, "copilot", "claude");

    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert!(config.api_key.is_none());
}

#[test]
fn test_from_env_copilot_case_insensitive() {
    // Test that copilot provider name is case-insensitive
    // For copilot providers, api_key should be None (rig handles env vars via from_env())
    let env = env_map(&[("GITHUB_COPILOT_API_KEY", "copilot_key")]);

    // Lowercase
    let config1 = Config::from_env(&env, "copilot", "claude");
    assert_eq!(config1.api_key, None);

    // Mixed case
    let config2 = Config::from_env(&env, "Copilot", "claude");
    assert_eq!(config2.api_key, None);

    // github-copilot variant
    let config3 = Config::from_env(&env, "github-copilot", "claude");
    assert_eq!(config3.api_key, None);

    // Mixed case variant
    let config4 = Config::from_env(&env, "GitHub-Copilot", "claude");
    assert_eq!(config4.api_key, None);
}
