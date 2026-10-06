use std::collections::HashMap;
use std::sync::Arc;

use tempfile::TempDir;

use super::{ModelRoleConfig, PluginConfig};
use crate::config::file_backend::FileBackend;
use crate::config::types::ProviderConfig;
use crate::config::vault::{Vault, VaultBackendKind};
use crate::test_support::env_map;
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_resolver_env_api_key_beats_provider_store_reference() -> Result<()> {
    // -- Setup & Fixtures
    // Provider config points at the vault; the env var supplies a raw key.
    let (_dir, vault) = temp_vault(&[("openai", "sk-from-vault")])?;
    let plugin_config = plugin_config_with("store:openai", Some(vault));
    let role_config = default_role("openai/gpt-4");
    let env = env_map(&[("OPENAI_API_KEY", "sk-from-env")]);

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(
        config.api_key.as_deref(),
        Some("sk-from-env"),
        "the env var must win over the provider's store: reference"
    );
    Ok(())
}

#[test]
fn test_resolver_store_reference_resolves_from_vault_when_env_absent() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault(&[("openai", "sk-from-vault")])?;
    let plugin_config = plugin_config_with("store:openai", Some(vault));
    let role_config = default_role("openai/gpt-4");
    let env = EnvMap::new();

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(
        config.api_key.as_deref(),
        Some("sk-from-vault"),
        "with no env var the store: reference resolves from the vault"
    );
    Ok(())
}

#[test]
fn test_resolver_raw_literal_api_key_is_used_as_is() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault(&[])?;
    let plugin_config = plugin_config_with("sk-raw-literal", Some(vault));
    let role_config = default_role("openai/gpt-4");
    let env = EnvMap::new();

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(
        config.api_key.as_deref(),
        Some("sk-raw-literal"),
        "a literal key is never treated as a vault lookup"
    );
    Ok(())
}

#[test]
fn test_resolver_store_reference_without_vault_is_left_unresolved() -> Result<()> {
    // -- Setup & Fixtures
    // No vault configured, so nothing can resolve the reference.
    let plugin_config = plugin_config_with("store:openai", None);
    let role_config = default_role("openai/gpt-4");
    let env = EnvMap::new();

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(
        config.api_key.as_deref(),
        Some("store:openai"),
        "without a vault the reference is passed through unchanged"
    );
    Ok(())
}

#[test]
fn test_resolver_role_overrides_apply_after_store_resolution() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault(&[("openai", "sk-from-vault")])?;
    let plugin_config = plugin_config_with("store:openai", Some(vault));
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        temperature: Some(0.25),
        max_tokens: Some(777),
        ..ModelRoleConfig::default()
    };
    let env = EnvMap::new();

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(config.api_key.as_deref(), Some("sk-from-vault"));
    assert_eq!(config.temperature, Some(0.25), "role overrides still apply");
    assert_eq!(config.max_tokens, Some(777), "role overrides still apply");
    Ok(())
}

#[test]
fn test_resolver_env_reference_resolves_from_env_var() -> Result<()> {
    // -- Setup & Fixtures
    // The provider config names a custom env var via the `env:` reference.
    let (_dir, vault) = temp_vault(&[])?;
    let plugin_config = plugin_config_with("env:MY_CUSTOM_PROVIDER_KEY", Some(vault));
    let role_config = default_role("openai/gpt-4");
    let env = env_map(&[("MY_CUSTOM_PROVIDER_KEY", "sk-from-custom-env")]);

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(
        config.api_key.as_deref(),
        Some("sk-from-custom-env"),
        "an env: reference resolves to the env var value"
    );
    Ok(())
}

#[test]
fn test_resolver_env_reference_missing_env_var_sets_none() -> Result<()> {
    // -- Setup & Fixtures
    let (_dir, vault) = temp_vault(&[])?;
    let plugin_config = plugin_config_with("env:MISSING_PROVIDER_KEY_XYZ", Some(vault));
    let role_config = default_role("openai/gpt-4");
    let env = EnvMap::new();

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(
        config.api_key, None,
        "an unresolved env: reference must not leak the reference string"
    );
    Ok(())
}

#[test]
fn test_resolver_omitted_api_key_defaults_to_vault_by_provider_name() -> Result<()> {
    // -- Setup & Fixtures
    // No api_key field in the provider config; the vault holds provider:openai.
    let (_dir, vault) = temp_vault(&[("openai", "sk-from-default-vault")])?;
    let plugin_config = plugin_config_without_api_key(Some(vault));
    let role_config = default_role("openai/gpt-4");
    let env = EnvMap::new();

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(
        config.api_key.as_deref(),
        Some("sk-from-default-vault"),
        "an omitted api_key defaults to the vault entry for the provider name"
    );
    Ok(())
}

#[test]
fn test_resolver_omitted_api_key_no_vault_entry_sets_none() -> Result<()> {
    // -- Setup & Fixtures
    // No api_key field and no vault entry for the provider.
    let (_dir, vault) = temp_vault(&[])?;
    let plugin_config = plugin_config_without_api_key(Some(vault));
    let role_config = default_role("openai/gpt-4");
    let env = EnvMap::new();

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(
        config.api_key, None,
        "with no vault entry the api_key stays None"
    );
    Ok(())
}

#[test]
fn test_resolver_env_api_key_beats_default_vault_lookup() -> Result<()> {
    // -- Setup & Fixtures
    // api_key omitted; vault holds a key, but the env var must still win.
    let (_dir, vault) = temp_vault(&[("openai", "sk-from-vault")])?;
    let plugin_config = plugin_config_without_api_key(Some(vault));
    let role_config = default_role("openai/gpt-4");
    let env = env_map(&[("OPENAI_API_KEY", "sk-from-env")]);

    // -- Exec
    let resolved = plugin_config.resolve_model_with(&role_config, &env);

    // -- Check
    let config = resolved.map_err(|e| format!("should resolve: {e}"))?;
    assert_eq!(
        config.api_key.as_deref(),
        Some("sk-from-env"),
        "the env var must win over the default vault lookup"
    );
    Ok(())
}

// endregion: --- Tests

// region:    --- Test Support

/// A `FileBackend`-backed vault seeded with `provider name -> key` pairs.
fn temp_vault(providers: &[(&str, &str)]) -> Result<(TempDir, Arc<Vault>)> {
    let dir = TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    let vault = Vault::new(VaultBackendKind::File(backend));
    for (name, key) in providers {
        vault.store_api_key(name, key)?;
    }
    Ok((dir, Arc::new(vault)))
}

/// A `PluginConfig` with one provider whose `api_key` is `api_key`.
fn plugin_config_with(api_key: &str, vault: Option<Arc<Vault>>) -> PluginConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            api_key: Some(api_key.to_string()),
            ..ProviderConfig::default()
        },
    );
    PluginConfig {
        providers,
        vault,
        ..PluginConfig::default()
    }
}

/// The default role pointing at `spec`.
fn default_role(spec: &str) -> ModelRoleConfig {
    ModelRoleConfig {
        model: spec.to_string(),
        ..ModelRoleConfig::default()
    }
}

/// A `PluginConfig` with one provider whose `api_key` is omitted entirely.
fn plugin_config_without_api_key(vault: Option<Arc<Vault>>) -> PluginConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            api_key: None,
            ..ProviderConfig::default()
        },
    );
    PluginConfig {
        providers,
        vault,
        ..PluginConfig::default()
    }
}

// endregion: --- Test Support
