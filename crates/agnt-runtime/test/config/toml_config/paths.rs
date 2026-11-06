//! `validate_paths` pin: every serde key of `PathsConfig` is validated.

use crate::config::paths::PathsConfig;
use tempfile::TempDir;

use super::*;

/// The `[paths]` keys `PathsConfig` serializes, discovered at runtime.
///
/// The template is an exhaustive struct literal: adding a field to
/// `PathsConfig` stops this file from compiling until the field is named here,
/// and `validate_paths` then has to cover the new key or the rejection test
/// below fails. Serializing the template and reading the resulting table's keys
/// keeps the key list free of hand-written entries, so it cannot drift from the
/// struct.
fn paths_config_keys() -> Result<Vec<String>> {
    let template = PathsConfig {
        data_home: Some("~".to_string()),
        cache_home: Some("~".to_string()),
        state_home: Some("~".to_string()),
    };
    let table: toml::Table = toml::to_string(&template)
        .map_err(|e| format!("the PathsConfig template should serialize: {e}"))?
        .parse()
        .map_err(|e| format!("the serialized template should parse into a table: {e}"))?;
    let keys = table.keys().cloned().collect::<Vec<_>>();
    if keys.is_empty() {
        return Err("the PathsConfig template serialized no keys".into());
    }
    Ok(keys)
}

/// A relative `[paths]` value is rejected for every field the struct serializes,
/// so a new field cannot slip through `validate_paths` untouched.
#[test]
fn test_toml_config_validate_paths_rejects_relative_value_for_every_key() -> Result<()> {
    // -- Setup & Fixtures
    let keys = paths_config_keys()?;

    for key in keys {
        // -- Exec
        let dir = TempDir::new()?;
        let path = write_config(&dir, &format!("[paths]\n{key} = \"rel/value\"\n"))?;
        let result = load_from(&path);

        // -- Check: the loader must reject, naming the key and the value.
        let Err(err) = result else {
            return Err(format!(
                "[paths].{key} = \"rel/value\" loaded successfully: add `{key}` to the `fields` array in validate_paths"
            )
            .into());
        };
        let TomlConfigError::Paths(message) = err else {
            return Err(
                format!("[paths].{key} should fail with TomlConfigError::Paths: {err:?}").into(),
            );
        };
        assert!(
            message.contains(&format!("[paths].{key}")),
            "message must name [paths].{key}: {message}"
        );
        assert!(
            message.contains("rel/value"),
            "message must name the value: {message}"
        );
    }
    Ok(())
}

/// An absolute `[paths]` value is accepted for every field the struct serializes.
#[test]
fn test_toml_config_validate_paths_accepts_absolute_value_for_every_key() -> Result<()> {
    // -- Setup & Fixtures
    let keys = paths_config_keys()?;

    for key in keys {
        // -- Exec
        let dir = TempDir::new()?;
        let path = write_config(&dir, &format!("[paths]\n{key} = \"/abs/value\"\n"))?;
        let config = load_from(&path)
            .map_err(|e| format!("[paths].{key} = \"/abs/value\" should load: {e:?}"))?;

        // -- Check
        assert!(
            config.paths.is_some(),
            "[paths].{key} should populate config.paths"
        );
    }
    Ok(())
}
