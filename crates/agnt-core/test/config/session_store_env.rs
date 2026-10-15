//! Session store type env var tests.

use crate::config::Config;
use crate::session::StoreType;
use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_session_store_env_var_parsing() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Option<StoreType>)> = vec![
        ("sqlite", Some(StoreType::Sqlite)),
        ("jsonl", Some(StoreType::Jsonl)),
        ("memory", Some(StoreType::Memory)),
        ("unknown", None),
    ];

    // -- Exec & Check
    for (value, expected) in cases {
        let env = env_map(&[("AGENT_SESSION_STORE_TYPE", value)]);
        let config = Config::from_env(&env, "openai", "gpt-4");
        assert_eq!(config.session_store_type, expected, "value {value}");
    }

    // -- Check: an unset env var leaves the field None
    let config = Config::from_env(&EnvMap::new(), "openai", "gpt-4");
    assert_eq!(config.session_store_type, None);
    Ok(())
}
