//! Copilot-specific `Config::from_env()` API-key tests.

use crate::config::Config;
use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_from_env_copilot_providers_never_set_api_key() -> Result<()> {
    // -- Setup & Fixtures
    // For copilot providers, api_key stays None — rig resolves the token from
    // the environment itself via `from_env()`.
    let cases: Vec<(&str, &[(&str, &str)])> = vec![
        ("copilot", &[("GITHUB_COPILOT_API_KEY", "copilot_key")]),
        ("copilot", &[("GITHUB_TOKEN", "github_token")]),
        (
            "copilot",
            &[
                ("GITHUB_COPILOT_API_KEY", "copilot_key"),
                ("GITHUB_TOKEN", "github_token"),
            ],
        ),
        ("copilot", &[]),
        ("Copilot", &[("GITHUB_COPILOT_API_KEY", "copilot_key")]),
        (
            "github-copilot",
            &[("GITHUB_COPILOT_API_KEY", "copilot_key")],
        ),
        (
            "GitHub-Copilot",
            &[("GITHUB_COPILOT_API_KEY", "copilot_key")],
        ),
    ];

    // -- Exec & Check
    for (provider, vars) in cases {
        let env = env_map(vars);
        let config = Config::from_env(&env, provider, "claude");
        assert_eq!(config.provider, provider);
        assert_eq!(config.model, "claude");
        assert_eq!(
            config.api_key, None,
            "provider {provider} must not set api_key"
        );
    }

    // -- Check: an empty env map is equivalent to no keys at all
    let config = Config::from_env(&EnvMap::new(), "copilot", "claude");
    assert!(config.api_key.is_none());
    Ok(())
}
