use serial_test::serial;

use crate::config::vault::VaultBackend;

use super::FileBackend;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Tests

#[test]
fn test_file_backend_set_then_get_round_trip() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));

    // -- Exec
    backend.set("key1", "val1")?;
    let fetched = backend.get("key1")?;

    // -- Check
    assert_eq!(fetched.as_deref(), Some("val1"));
    Ok(())
}

#[test]
fn test_file_backend_get_missing_key_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    backend.set("key1", "val1")?;

    // -- Exec
    let fetched = backend.get("nonexistent")?;

    // -- Check
    assert_eq!(fetched, None);
    Ok(())
}

#[test]
fn test_file_backend_get_when_file_absent_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));

    // -- Exec
    let fetched = backend.get("key")?;

    // -- Check
    assert_eq!(fetched, None, "a missing file reads as empty");
    Ok(())
}

#[test]
fn test_file_backend_delete_removes_key() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    backend.set("key1", "val1")?;

    // -- Exec
    backend.delete("key1")?;
    let fetched = backend.get("key1")?;

    // -- Check
    assert_eq!(fetched, None, "deleted key is gone");
    Ok(())
}

#[test]
fn test_file_backend_delete_missing_key_is_idempotent() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));

    // -- Exec
    let first = backend.delete("nonexistent");
    let second = backend.delete("nonexistent");

    // -- Check
    assert!(first.is_ok(), "deleting an absent key is not an error");
    assert!(second.is_ok(), "repeat delete is not an error");
    Ok(())
}

#[test]
fn test_file_backend_list_contains_all_keys() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));

    // -- Exec
    backend.set("key1", "v1")?;
    backend.set("key2", "v2")?;
    let listed = backend.list()?;

    // -- Check
    assert_eq!(listed.len(), 2, "both keys are listed");
    assert!(listed.contains(&"key1".to_string()));
    assert!(listed.contains(&"key2".to_string()));
    Ok(())
}

#[test]
fn test_file_backend_list_drops_deleted_key() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let backend = FileBackend::new(dir.path().join("secrets.json"));
    backend.set("key1", "v1")?;
    backend.set("key2", "v2")?;

    // -- Exec
    backend.delete("key1")?;
    let listed = backend.list()?;

    // -- Check
    assert!(!listed.contains(&"key1".to_string()), "key1 is dropped");
    assert!(listed.contains(&"key2".to_string()), "key2 remains");
    Ok(())
}

#[test]
fn test_file_backend_set_persists_across_instances() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let path = dir.path().join("secrets.json");
    let writer = FileBackend::new(path.clone());

    // -- Exec
    writer.set("key1", "persisted")?;
    let reader = FileBackend::new(path);
    let fetched = reader.get("key1")?;

    // -- Check
    assert_eq!(fetched.as_deref(), Some("persisted"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_file_backend_set_uses_0600_permissions() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let path = dir.path().join("secrets.json");
    let backend = FileBackend::new(path.clone());

    // -- Exec
    backend.set("key1", "val1")?;
    let mode = std::fs::metadata(&path)?.permissions().mode();

    // -- Check
    assert_eq!(mode & 0o777, 0o600, "secrets file must be 0600");
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_file_backend_set_tightens_loose_permissions() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let path = dir.path().join("secrets.json");
    std::fs::write(&path, "{}")?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
    let backend = FileBackend::new(path.clone());

    // -- Exec
    backend.set("key1", "val1")?;
    let mode = std::fs::metadata(&path)?.permissions().mode();

    // -- Check
    assert_eq!(
        mode & 0o777,
        0o600,
        "a pre-existing loose file is tightened"
    );
    Ok(())
}

#[test]
fn test_file_backend_set_writes_valid_json_map() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let path = dir.path().join("secrets.json");
    let backend = FileBackend::new(path.clone());

    // -- Exec
    backend.set("provider:openai", "sk-test")?;
    let raw = std::fs::read_to_string(&path)?;
    let map: std::collections::HashMap<String, String> = serde_json::from_str(&raw)?;

    // -- Check
    assert_eq!(
        map.get("provider:openai").map(String::as_str),
        Some("sk-test"),
        "file holds a key -> value JSON map"
    );
    Ok(())
}

#[test]
fn test_file_backend_concurrent_sets_do_not_lose_keys() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let path = dir.path().join("secrets.json");

    // -- Exec
    let mut handles = Vec::new();
    for index in 0..8 {
        let path = path.clone();
        handles.push(std::thread::spawn(move || {
            let backend = FileBackend::new(path);
            backend.set(&format!("key{index}"), &format!("val{index}"))
        }));
    }
    for handle in handles {
        handle.join().map_err(|_| "worker thread panicked")??;
    }

    // -- Check
    let backend = FileBackend::new(path);
    let listed = backend.list()?;
    assert_eq!(listed.len(), 8, "every concurrent set is durable");
    Ok(())
}

#[test]
#[serial]
fn test_file_backend_probe_succeeds_with_writable_data_dir() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let previous = std::env::var_os("XDG_DATA_HOME");
    unsafe {
        std::env::set_var("XDG_DATA_HOME", dir.path());
    }

    // -- Exec
    let probed = FileBackend::probe();

    // -- Cleanup
    unsafe {
        match previous {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
    }

    // -- Check
    let backend = probed.map_err(|e| format!("probe should succeed: {e}"))?;
    let path = backend.path().to_string_lossy();
    assert!(
        path.contains("nu-agent"),
        "probe path lives under nu-agent/"
    );
    assert!(
        path.contains("secrets.json"),
        "probe path points at secrets.json"
    );
    Ok(())
}

#[test]
#[serial]
fn test_file_backend_default_path_uses_xdg_data_home() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::TempDir::new()?;
    let previous = std::env::var_os("XDG_DATA_HOME");
    unsafe {
        std::env::set_var("XDG_DATA_HOME", dir.path());
    }

    // -- Exec
    let resolved = FileBackend::default_path();

    // -- Cleanup
    unsafe {
        match previous {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
    }

    // -- Check
    let path = resolved.map_err(|e| format!("should resolve: {e}"))?;
    let expected = dir.path().join("nu-agent").join("secrets.json");
    assert_eq!(
        path, expected,
        "path is $XDG_DATA_HOME/nu-agent/secrets.json"
    );
    Ok(())
}

#[test]
#[serial]
fn test_file_backend_probe_fails_when_data_dir_not_writable() -> Result<()> {
    // -- Setup & Fixtures
    // Point XDG_DATA_HOME at a path whose parent is a regular file, so the
    // nu-agent/ subdirectory cannot be created.
    let dir = tempfile::TempDir::new()?;
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "not a directory")?;
    let previous = std::env::var_os("XDG_DATA_HOME");
    unsafe {
        std::env::set_var("XDG_DATA_HOME", blocker.join("nested"));
    }

    // -- Exec
    let probed = FileBackend::probe();

    // -- Cleanup
    unsafe {
        match previous {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
    }

    // -- Check
    assert!(
        matches!(probed, Err(super::VaultError::NoDataDir)),
        "an unusable data dir must report NoDataDir"
    );
    Ok(())
}

// endregion: --- Tests
