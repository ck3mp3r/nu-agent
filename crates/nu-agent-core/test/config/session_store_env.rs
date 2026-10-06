//! Session store type env var tests.

use crate::config::Config;
use crate::session::StoreType;
use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;

#[test]
fn test_session_store_env_var_sqlite() {
    let env = env_map(&[("AGENT_SESSION_STORE_TYPE", "sqlite")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.session_store_type, Some(StoreType::Sqlite));
}

#[test]
fn test_session_store_env_var_jsonl() {
    let env = env_map(&[("AGENT_SESSION_STORE_TYPE", "jsonl")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.session_store_type, Some(StoreType::Jsonl));
}

#[test]
fn test_session_store_env_var_memory() {
    let env = env_map(&[("AGENT_SESSION_STORE_TYPE", "memory")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.session_store_type, Some(StoreType::Memory));
}

#[test]
fn test_session_store_env_var_unknown_ignored() {
    // Invalid env var values are silently ignored (None) by from_env
    let env = env_map(&[("AGENT_SESSION_STORE_TYPE", "unknown")]);
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.session_store_type, None);
}

#[test]
fn test_session_store_env_var_not_set() {
    // When env var is not set, session_store_type should be None
    let env = EnvMap::new();
    let config = Config::from_env(&env, "openai", "gpt-4");
    assert_eq!(config.session_store_type, None);
}
