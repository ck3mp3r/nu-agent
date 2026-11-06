//! Rejected values and the config-discovery ladder.

use crate::config::paths::{Paths, PathsConfig, PathsError};
use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;
use crate::utils::xdg::XdgError;

use super::support::{Result, app_dir};

// region:    --- Tests

#[test]
fn test_paths_resolve_rejects_relative_env_homes() -> Result<()> {
    // -- Setup & Fixtures
    let xdg = [
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_DATA_HOME", "/xdg/data"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ];
    let cases = [
        ("NU_AGENT_DATA_HOME", "rel/data"),
        ("NU_AGENT_CACHE_HOME", "rel/cache"),
        ("NU_AGENT_STATE_HOME", "rel/state"),
    ];
    for (env_key, value) in cases {
        let mut entries = xdg.to_vec();
        entries.push((env_key, value));
        let env = env_map(&entries);

        // -- Exec
        let error = match Paths::resolve_for_process_with(None, &env) {
            Ok(_) => return Err(format!("{env_key} = '{value}' must be rejected").into()),
            Err(error) => error,
        };

        // -- Check: the error names the variable and the rejected value
        assert!(
            matches!(&error, PathsError::RelativeValue { label, value: rejected }
                if label == env_key && rejected == value),
            "the error names {env_key} and its value: {error}"
        );
        assert!(
            error.to_string().contains("relative"),
            "the error says the value is relative: {error}"
        );
    }
    Ok(())
}

#[test]
fn test_paths_resolve_rejects_relative_config_homes() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[
        ("XDG_DATA_HOME", "/xdg/data"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);
    let config_dir = app_dir("/loaded");
    let cases = [
        (
            "[paths].data_home",
            PathsConfig {
                data_home: Some("rel/data".to_string()),
                ..PathsConfig::default()
            },
        ),
        (
            "[paths].cache_home",
            PathsConfig {
                cache_home: Some("rel/cache".to_string()),
                ..PathsConfig::default()
            },
        ),
        (
            "[paths].state_home",
            PathsConfig {
                state_home: Some("rel/state".to_string()),
                ..PathsConfig::default()
            },
        ),
    ];
    for (field, config) in cases {
        // -- Exec
        let error = match Paths::resolve_with(Some(&config), &config_dir, &env) {
            Ok(_) => return Err(format!("the relative {field} must be rejected").into()),
            Err(error) => error,
        };

        // -- Check: the error names the field
        assert!(
            matches!(&error, PathsError::RelativeValue { label, .. } if label == field),
            "the error names {field}: {error}"
        );
        assert!(
            error.to_string().contains(field),
            "the message names the field: {error}"
        );
    }
    Ok(())
}

#[test]
fn test_paths_bootstrap_rejects_relative_config_home() -> Result<()> {
    // -- Setup & Fixtures
    let env = env_map(&[
        ("NU_AGENT_CONFIG_HOME", "rel/config"),
        ("XDG_CONFIG_HOME", "/xdg/config"),
    ]);

    // -- Exec
    let error = match Paths::bootstrap_config_path_with(&env) {
        Ok(path) => {
            return Err(
                format!("a relative NU_AGENT_CONFIG_HOME must be rejected: {path:?}").into(),
            );
        }
        Err(error) => error,
    };

    // -- Check
    assert!(
        matches!(&error, PathsError::RelativeValue { label, .. } if label == "NU_AGENT_CONFIG_HOME"),
        "the error names the variable: {error}"
    );
    assert!(
        error.to_string().contains("NU_AGENT_CONFIG_HOME"),
        "the message names the variable: {error}"
    );
    Ok(())
}

#[test]
fn test_paths_resolve_reports_missing_home() -> Result<()> {
    // -- Setup & Fixtures
    let env = EnvMap::new();

    // -- Exec
    let error = match Paths::resolve_for_process_with(None, &env) {
        Ok(_) => {
            return Err("resolution should fail when HOME and the XDG bases are missing".into());
        }
        Err(error) => error,
    };

    // -- Check
    assert!(matches!(error, PathsError::Xdg(XdgError::HomeNotFound)));

    // -- Check: a tilde value needs HOME even when every base is injected
    let tilde_config = PathsConfig {
        data_home: Some("~/data".to_string()),
        ..PathsConfig::default()
    };
    let injected = env_map(&[
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);
    let error = match Paths::resolve_for_process_with(Some(&tilde_config), &injected) {
        Ok(_) => return Err("a tilde value should fail when HOME is missing".into()),
        Err(error) => error,
    };
    assert!(matches!(error, PathsError::HomeNotFound));
    Ok(())
}

#[test]
fn test_paths_bootstrap_follows_the_home_ladder() -> Result<()> {
    // -- Setup & Fixtures
    let overridden = env_map(&[
        ("HOME", "/home/tester"),
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("NU_AGENT_CONFIG_HOME", "/custom/etc"),
    ]);
    let xdg_only = env_map(&[("HOME", "/home/tester"), ("XDG_CONFIG_HOME", "/xdg/config")]);
    let home_only = env_map(&[("HOME", "/home/tester")]);

    // -- Exec & Check: env beats XDG beats the HOME default
    assert_eq!(
        Paths::bootstrap_config_path_with(&overridden)?,
        app_dir("/custom/etc").join("config.toml")
    );
    assert_eq!(
        Paths::bootstrap_config_path_with(&xdg_only)?,
        app_dir("/xdg/config").join("config.toml")
    );
    assert_eq!(
        Paths::bootstrap_config_path_with(&home_only)?,
        app_dir("/home/tester/.config").join("config.toml")
    );
    Ok(())
}

// endregion: --- Tests
