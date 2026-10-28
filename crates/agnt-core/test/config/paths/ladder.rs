//! The precedence ladder: env home, then `[paths]` home, then XDG, then HOME.

use std::path::PathBuf;

use crate::config::copilot_tokens::copilot_dir_name;
use crate::config::paths::{Paths, PathsConfig};
use crate::test_support::env_map;
use crate::utils::paths::APP_DIR_NAME;

use super::support::{EXPECTED_APP_DIR, Result, app_dir};

// region:    --- Tests

#[test]
fn test_paths_resolve_env_homes_win_over_config_homes() -> Result<()> {
    // -- Setup & Fixtures
    let config = PathsConfig {
        data_home: Some("/from/config/data".to_string()),
        cache_home: Some("/from/config/cache".to_string()),
        state_home: Some("/from/config/state".to_string()),
    };
    let env = env_map(&[
        ("NU_AGENT_CONFIG_HOME", "/env/config"),
        ("NU_AGENT_DATA_HOME", "/env/data"),
        ("NU_AGENT_CACHE_HOME", "/env/cache"),
        ("NU_AGENT_STATE_HOME", "/env/state"),
    ]);

    // -- Exec
    let paths =
        Paths::resolve_for_process_with(Some(&config), &env).map_err(|error| error.to_string())?;

    // -- Check: every tree re-roots under the env value and keeps its tail
    let config_app_dir = app_dir("/env/config");
    let data_app_dir = app_dir("/env/data");
    let cache_app_dir = app_dir("/env/cache");
    let log_app_dir = app_dir("/env/state").join("logs");

    assert_eq!(paths.config_path, config_app_dir.join("config.toml"));
    assert_eq!(paths.dotenv_path, config_app_dir.join(".env"));
    assert_eq!(paths.agents_dir, config_app_dir.join("agents"));
    assert_eq!(
        paths.copilot_token_dir,
        config_app_dir.join(copilot_dir_name())
    );
    assert_eq!(paths.store_path, cache_app_dir.join("rig-sessions.db"));
    assert_eq!(
        paths.grammar_cache_dir,
        PathBuf::from("/env/cache").join("tree-sitter")
    );
    assert_eq!(paths.log_dir, log_app_dir);
    assert_eq!(paths.log_path, log_app_dir.join("agent.log"));
    assert_eq!(paths.secrets_path, data_app_dir.join("secrets.json"));
    assert_eq!(paths.models_cache_path, data_app_dir.join("models.json"));
    assert_eq!(paths.theme_path, data_app_dir.join("theme.json"));
    assert_eq!(paths.audit_log_path, data_app_dir.join("tool_audit.log"));

    // -- Check: no config value leaks past the higher level
    assert!(!paths.store_path.starts_with("/from/config"));
    assert!(!paths.config_path.starts_with("/from/config"));
    Ok(())
}

#[test]
fn test_paths_resolve_config_homes_used_when_env_unset() -> Result<()> {
    // -- Setup & Fixtures
    let config = PathsConfig {
        data_home: Some("/from/config/data".to_string()),
        cache_home: Some("/from/config/cache".to_string()),
        state_home: Some("/from/config/state".to_string()),
    };
    let env = env_map(&[
        ("XDG_DATA_HOME", "/xdg/data"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);
    let config_dir = app_dir("/loaded");

    // -- Exec
    let paths =
        Paths::resolve_with(Some(&config), &config_dir, &env).map_err(|error| error.to_string())?;

    // -- Check: the config quartet hangs off the loaded config dir
    assert_eq!(paths.config_path, config_dir.join("config.toml"));
    assert_eq!(paths.agents_dir, config_dir.join("agents"));

    // -- Check: each base override re-roots its tree keeping APP_DIR_NAME
    assert_eq!(
        paths.secrets_path,
        app_dir("/from/config/data").join("secrets.json")
    );
    assert_eq!(
        paths.store_path,
        app_dir("/from/config/cache").join("rig-sessions.db")
    );
    // -- Check: the grammar cache follows the same cache winner, without
    // APP_DIR_NAME: the tree-sitter CLI shares that directory
    assert_eq!(
        paths.grammar_cache_dir,
        PathBuf::from("/from/config/cache").join("tree-sitter")
    );
    assert_eq!(
        paths.log_path,
        app_dir("/from/config/state").join("logs").join("agent.log")
    );
    assert_eq!(paths.log_dir, app_dir("/from/config/state").join("logs"));
    Ok(())
}

#[test]
fn test_paths_resolve_falls_back_to_xdg_tree() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_DATA_HOME", "/xdg/data"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);
    let config_dir = app_dir("/xdg/config");

    // -- Exec
    let paths = Paths::resolve_with(None, &config_dir, &env).map_err(|error| error.to_string())?;

    // -- Check: the -dev split applies to every derived path
    assert_eq!(APP_DIR_NAME, EXPECTED_APP_DIR);
    let config_app_dir = app_dir("/xdg/config");
    let data_app_dir = app_dir("/xdg/data");
    let cache_app_dir = app_dir("/xdg/cache");
    let log_app_dir = app_dir("/xdg/state").join("logs");

    assert_eq!(paths.config_path, config_app_dir.join("config.toml"));
    assert_eq!(paths.dotenv_path, config_app_dir.join(".env"));
    assert_eq!(paths.agents_dir, config_app_dir.join("agents"));
    assert_eq!(
        paths.copilot_token_dir,
        config_app_dir.join(copilot_dir_name())
    );
    assert_eq!(paths.store_path, cache_app_dir.join("rig-sessions.db"));
    assert_eq!(paths.log_dir, log_app_dir);
    assert_eq!(paths.log_path, log_app_dir.join("agent.log"));
    assert_eq!(paths.secrets_path, data_app_dir.join("secrets.json"));
    assert_eq!(paths.models_cache_path, data_app_dir.join("models.json"));
    assert_eq!(paths.theme_path, data_app_dir.join("theme.json"));
    assert_eq!(paths.audit_log_path, data_app_dir.join("tool_audit.log"));
    assert_eq!(
        paths.grammar_cache_dir,
        PathBuf::from("/xdg/cache").join("tree-sitter")
    );
    Ok(())
}

#[test]
fn test_paths_resolve_expands_leading_tilde() -> Result<()> {
    // -- Setup & Fixtures
    let config = PathsConfig {
        data_home: Some("~/data".to_string()),
        ..PathsConfig::default()
    };
    let env = env_map(&[
        ("HOME", "/home/tester"),
        ("NU_AGENT_CONFIG_HOME", "~/.local/etc"),
        ("NU_AGENT_CACHE_HOME", "~/cache"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);

    // -- Exec
    let paths =
        Paths::resolve_for_process_with(Some(&config), &env).map_err(|error| error.to_string())?;

    // -- Check
    assert_eq!(
        paths.config_path,
        app_dir("/home/tester/.local/etc").join("config.toml")
    );
    assert_eq!(
        paths.store_path,
        app_dir("/home/tester/cache").join("rig-sessions.db")
    );
    assert_eq!(
        paths.secrets_path,
        app_dir("/home/tester/data").join("secrets.json")
    );
    assert_eq!(
        paths.log_path,
        app_dir("/xdg/state").join("logs").join("agent.log")
    );
    Ok(())
}

#[test]
fn test_paths_resolve_empty_values_count_as_absent() -> Result<()> {
    // -- Setup & Fixtures
    let config = PathsConfig {
        data_home: Some(String::new()),
        cache_home: Some(String::new()),
        state_home: Some(String::new()),
    };
    let env = env_map(&[
        ("NU_AGENT_DATA_HOME", ""),
        ("NU_AGENT_CACHE_HOME", ""),
        ("NU_AGENT_STATE_HOME", ""),
        ("XDG_DATA_HOME", "/xdg/data"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);
    let config_dir = app_dir("/loaded");

    // -- Exec
    let paths =
        Paths::resolve_with(Some(&config), &config_dir, &env).map_err(|error| error.to_string())?;

    // -- Check: an empty value falls through to the XDG base
    assert_eq!(
        paths.secrets_path,
        app_dir("/xdg/data").join("secrets.json")
    );
    assert_eq!(
        paths.store_path,
        app_dir("/xdg/cache").join("rig-sessions.db")
    );
    assert_eq!(
        paths.log_path,
        app_dir("/xdg/state").join("logs").join("agent.log")
    );
    Ok(())
}

#[test]
fn test_paths_resolve_for_process_with_derives_the_config_quartet_from_bootstrap() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[
        ("NU_AGENT_CONFIG_HOME", "/custom/etc"),
        ("XDG_DATA_HOME", "/xdg/data"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);
    let config = PathsConfig {
        data_home: Some("/from/config/data".to_string()),
        cache_home: Some("/from/config/cache".to_string()),
        state_home: Some("/from/config/state".to_string()),
    };

    // -- Exec
    let paths =
        Paths::resolve_for_process_with(Some(&config), &env).map_err(|error| error.to_string())?;

    // -- Check: the config quartet follows the discovered config file, with no
    // `[paths]` gate, and the other trees keep their own ladders
    let config_app_dir = app_dir("/custom/etc");
    assert_eq!(paths.config_path, config_app_dir.join("config.toml"));
    assert_eq!(paths.dotenv_path, config_app_dir.join(".env"));
    assert_eq!(paths.agents_dir, config_app_dir.join("agents"));
    assert_eq!(
        paths.secrets_path,
        app_dir("/from/config/data").join("secrets.json")
    );
    assert_eq!(
        paths.store_path,
        app_dir("/from/config/cache").join("rig-sessions.db")
    );
    Ok(())
}

// endregion: --- Tests
