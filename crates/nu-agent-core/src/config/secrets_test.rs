use serial_test::serial;
use tempfile::TempDir;

use super::{Credential, SecretStore};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Helper to run a test with XDG_DATA_HOME pointed at a temp dir.
fn with_data_dir<F>(test: F) -> Result<()>
where
    F: FnOnce(&TempDir) -> Result<()>,
{
    let dir = TempDir::new().map_err(|e| format!("temp dir: {e}"))?;
    unsafe {
        std::env::set_var("XDG_DATA_HOME", dir.path());
    }
    let result = test(&dir);
    unsafe {
        std::env::remove_var("XDG_DATA_HOME");
    }
    result
}

#[test]
#[serial]
fn load_returns_empty_store_when_file_does_not_exist() -> Result<()> {
    with_data_dir(|_dir| {
        let store = SecretStore::load().map_err(|e| format!("load: {e}"))?;
        assert!(store.secrets.is_empty(), "expected empty store");
        Ok(())
    })
}

#[test]
#[serial]
fn save_and_load_round_trip() -> Result<()> {
    with_data_dir(|_dir| {
        let mut store = SecretStore::load().map_err(|e| format!("load: {e}"))?;
        store.set(
            "openai".to_string(),
            Credential::ApiKey {
                key: "sk-test-123".to_string(),
            },
        );
        store.save().map_err(|e| format!("save: {e}"))?;

        let loaded = SecretStore::load().map_err(|e| format!("reload: {e}"))?;
        assert_eq!(loaded.secrets.len(), 1);
        match loaded.get("openai") {
            Some(Credential::ApiKey { key }) => assert_eq!(key, "sk-test-123"),
            other => panic!("expected ApiKey, got: {other:?}"),
        }
        Ok(())
    })
}

#[test]
#[serial]
fn resolve_store_reference_returns_api_key() -> Result<()> {
    with_data_dir(|_dir| {
        let mut store = SecretStore::load().map_err(|e| format!("load: {e}"))?;
        store.set(
            "openai".to_string(),
            Credential::ApiKey {
                key: "sk-resolve-456".to_string(),
            },
        );

        let resolved = store.resolve("store:openai");
        assert_eq!(resolved.as_deref(), Some("sk-resolve-456"));
        Ok(())
    })
}

#[test]
#[serial]
fn resolve_returns_none_for_unknown_key() -> Result<()> {
    with_data_dir(|_dir| {
        let store = SecretStore::load().map_err(|e| format!("load: {e}"))?;
        assert_eq!(store.resolve("store:unknown"), None);
        assert_eq!(store.resolve("not-a-store-ref"), None);
        Ok(())
    })
}

#[test]
#[serial]
fn oauth_credential_storage_and_resolution() -> Result<()> {
    with_data_dir(|_dir| {
        let mut store = SecretStore::load().map_err(|e| format!("load: {e}"))?;
        store.set(
            "github-copilot".to_string(),
            Credential::OAuth {
                access_token: "gho-access-token".to_string(),
                refresh_token: Some("ghr-refresh-token".to_string()),
                expires_at: Some(1_700_000_000),
            },
        );
        store.save().map_err(|e| format!("save: {e}"))?;

        let loaded = SecretStore::load().map_err(|e| format!("reload: {e}"))?;
        match loaded.get("github-copilot") {
            Some(Credential::OAuth {
                access_token,
                refresh_token,
                expires_at,
            }) => {
                assert_eq!(access_token, "gho-access-token");
                assert_eq!(refresh_token.as_deref(), Some("ghr-refresh-token"));
                assert_eq!(*expires_at, Some(1_700_000_000));
            }
            other => panic!("expected OAuth, got: {other:?}"),
        }

        // resolve returns the access token for OAuth credentials
        let resolved = loaded.resolve("store:github-copilot");
        assert_eq!(resolved.as_deref(), Some("gho-access-token"));
        Ok(())
    })
}

#[test]
#[serial]
fn remove_deletes_entry() -> Result<()> {
    with_data_dir(|_dir| {
        let mut store = SecretStore::load().map_err(|e| format!("load: {e}"))?;
        store.set(
            "openai".to_string(),
            Credential::ApiKey {
                key: "sk-remove-me".to_string(),
            },
        );
        store.save().map_err(|e| format!("save: {e}"))?;

        let mut loaded = SecretStore::load().map_err(|e| format!("reload: {e}"))?;
        let removed = loaded.remove("openai");
        assert!(removed.is_some(), "remove should return the credential");
        assert!(loaded.get("openai").is_none(), "entry should be gone");

        // Removing a non-existent key returns None
        assert!(loaded.remove("nonexistent").is_none());
        Ok(())
    })
}

#[test]
#[serial]
fn list_returns_all_entries() -> Result<()> {
    with_data_dir(|_dir| {
        let mut store = SecretStore::load().map_err(|e| format!("load: {e}"))?;
        store.set(
            "openai".to_string(),
            Credential::ApiKey {
                key: "sk-1".to_string(),
            },
        );
        store.set(
            "anthropic".to_string(),
            Credential::ApiKey {
                key: "sk-ant-2".to_string(),
            },
        );

        let entries = store.list();
        assert_eq!(entries.len(), 2);
        let keys: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"openai"));
        assert!(keys.contains(&"anthropic"));
        Ok(())
    })
}

#[test]
#[serial]
fn path_uses_xdg_data_home() -> Result<()> {
    with_data_dir(|_dir| {
        let path = SecretStore::path().map_err(|e| format!("path: {e}"))?;
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("nu-agent"),
            "path should contain nu-agent: {path_str}"
        );
        assert!(
            path_str.contains("secrets.json"),
            "path should end with secrets.json: {path_str}"
        );
        Ok(())
    })
}

#[cfg(unix)]
#[test]
#[serial]
fn save_sets_0600_permissions() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    with_data_dir(|_dir| {
        let mut store = SecretStore::load().map_err(|e| format!("load: {e}"))?;
        store.set(
            "openai".to_string(),
            Credential::ApiKey {
                key: "sk-perm".to_string(),
            },
        );
        store.save().map_err(|e| format!("save: {e}"))?;

        let path = SecretStore::path().map_err(|e| format!("path: {e}"))?;
        let metadata = std::fs::metadata(&path).map_err(|e| format!("metadata: {e}"))?;
        let mode = metadata.permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "expected 0600 permissions");
        Ok(())
    })
}

#[test]
#[serial]
fn serialized_format_uses_type_tag() -> Result<()> {
    with_data_dir(|_dir| {
        let mut store = SecretStore::load().map_err(|e| format!("load: {e}"))?;
        store.set(
            "openai".to_string(),
            Credential::ApiKey {
                key: "sk-tag".to_string(),
            },
        );
        store.set(
            "github-copilot".to_string(),
            Credential::OAuth {
                access_token: "gho-tag".to_string(),
                refresh_token: None,
                expires_at: None,
            },
        );
        store.save().map_err(|e| format!("save: {e}"))?;

        let path = SecretStore::path().map_err(|e| format!("path: {e}"))?;
        let content = std::fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?;
        assert!(
            content.contains("\"type\": \"ApiKey\""),
            "missing ApiKey tag"
        );
        assert!(content.contains("\"type\": \"OAuth\""), "missing OAuth tag");
        Ok(())
    })
}
