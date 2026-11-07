//! Provider-specific `Config::from_env()` API-key behavior tests.

use crate::config::Config;
use crate::test_support::env_map;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_non_copilot_provider_sets_api_key_from_env() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[("OPENAI_API_KEY", "sk-test123")]);

    // -- Exec
    let config = Config::from_env(&env, "openai", "gpt-4");

    // -- Check
    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    assert!(config.additional_params.is_none(), "default is None");
    Ok(())
}
