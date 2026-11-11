//! The `[paths]` config block: what it parses and what it rejects.

use crate::config::PluginConfig;

use super::support::Result;

// region:    --- Tests

#[test]
fn test_paths_config_parses_from_config_toml() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("config.toml");
    std::fs::write(
        &path,
        concat!(
            "[paths]\n",
            "data_home = \"/var/lib/nu-agent\"\n",
            "cache_home = \"~/cache/nu-agent\"\n",
            "state_home = \"/var/state/nu-agent\"\n",
        ),
    )?;

    // -- Exec
    let config: PluginConfig = crate::config::toml_config::load_from(&path)?;
    let paths = config
        .paths
        .ok_or("the [paths] block should parse into PathsConfig")?;

    // -- Check
    assert_eq!(paths.data_home.as_deref(), Some("/var/lib/nu-agent"));
    assert_eq!(paths.cache_home.as_deref(), Some("~/cache/nu-agent"));
    assert_eq!(paths.state_home.as_deref(), Some("/var/state/nu-agent"));
    Ok(())
}

#[test]
fn test_paths_config_absent_when_block_missing() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("config.toml");
    std::fs::write(&path, "theme = \"catppuccin-latte\"\n")?;

    // -- Exec
    let config: PluginConfig = crate::config::toml_config::load_from(&path)?;

    // -- Check
    assert!(config.paths.is_none());
    Ok(())
}

#[test]
fn test_paths_config_rejects_relative_home_keys() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        ("[paths].data_home", "data_home", "rel/data"),
        ("[paths].cache_home", "cache_home", "rel/cache"),
        ("[paths].state_home", "state_home", "rel/state"),
    ];
    for (field, key, value) in cases {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("config.toml");
        std::fs::write(&path, format!("[paths]\n{key} = \"{value}\"\n"))?;

        // -- Exec
        let error = match crate::config::toml_config::load_from(&path) {
            Ok(_) => return Err(format!("a relative {field} must be rejected").into()),
            Err(error) => error,
        };

        // -- Check: the error names the field and says why
        assert!(
            error.to_string().contains(field),
            "the error names the field: {error}"
        );
        assert!(
            error.to_string().contains("relative"),
            "the error says the value is relative: {error}"
        );
    }
    Ok(())
}

#[test]
fn test_paths_config_accepts_absolute_and_tilde_homes() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("config.toml");
    std::fs::write(
        &path,
        concat!(
            "[paths]\n",
            "data_home = \"/var/lib/nu-agent\"\n",
            "cache_home = \"~\"\n",
            "state_home = \"~/.local/state\"\n",
        ),
    )?;

    // -- Exec
    let config: PluginConfig = crate::config::toml_config::load_from(&path)?;

    // -- Check
    let paths = config.paths.ok_or("the [paths] block parses")?;
    assert_eq!(paths.data_home.as_deref(), Some("/var/lib/nu-agent"));
    assert_eq!(paths.cache_home.as_deref(), Some("~"));
    assert_eq!(paths.state_home.as_deref(), Some("~/.local/state"));
    Ok(())
}

#[test]
fn test_paths_config_rejects_unknown_keys() -> Result<()> {
    // -- Setup & Fixtures: the six pre-swap keys plus a key that never existed
    let cases = [
        "config_path",
        "cache_path",
        "log_path",
        "data_dir",
        "cache_dir",
        "state_dir",
        "bogus_key",
    ];
    for key in cases {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("config.toml");
        std::fs::write(&path, format!("[paths]\n{key} = \"/tmp/ignored\"\n"))?;

        // -- Exec
        let error = match crate::config::toml_config::load_from(&path) {
            Ok(_) => return Err(format!("an unknown [paths].{key} must be rejected").into()),
            Err(error) => error,
        };

        // -- Check: the error names the unknown key and the keys that are
        // accepted, so a stale key cannot silently override nothing
        assert!(
            error.to_string().contains(key),
            "the error names the unknown key {key}: {error}"
        );
        assert!(
            error.to_string().contains("data_home"),
            "the error lists the accepted keys: {error}"
        );
    }
    Ok(())
}

// endregion: --- Tests
