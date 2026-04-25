use super::session::SessionStore;
use std::env;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_session_store_initializes_cache_directory() {
    // Arrange: Create a temporary test directory
    let temp_dir = env::temp_dir().join("nu-agent-test-init");
    let cache_dir = temp_dir.join("sessions");

    // Clean up if exists from previous test
    let _ = fs::remove_dir_all(&temp_dir);

    // Ensure directory doesn't exist before test
    assert!(
        !cache_dir.exists(),
        "Cache directory should not exist before initialization"
    );

    // Act: Create SessionStore with custom cache directory
    let store = SessionStore::new_with_cache_dir(cache_dir.clone());

    // Assert: Directory should be created
    assert!(
        cache_dir.exists(),
        "Cache directory should exist after initialization"
    );
    assert!(cache_dir.is_dir(), "Cache path should be a directory");
    assert_eq!(
        store.cache_dir(),
        &cache_dir,
        "Store should use the provided cache directory"
    );

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_session_store_uses_xdg_cache_home_if_set() {
    // Arrange: Set up a temporary XDG_CACHE_HOME
    let temp_dir = env::temp_dir().join("nu-agent-test-xdg");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    let expected_cache_dir = temp_dir.join("nu-agent").join("sessions");

    // Act: Create SessionStore with XDG_CACHE_HOME set
    let store = SessionStore::new_with_xdg_override(Some(temp_dir.clone()));

    // Assert: Should use XDG_CACHE_HOME/nu-agent/sessions
    assert_eq!(
        store.cache_dir(),
        &expected_cache_dir,
        "Should use XDG_CACHE_HOME/nu-agent/sessions"
    );
    assert!(
        expected_cache_dir.exists(),
        "XDG cache directory should be created"
    );

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_session_store_uses_default_when_xdg_cache_home_not_set() {
    // Arrange: Calculate expected directory based on XDG_CACHE_HOME or platform default
    let expected_base = env::var("XDG_CACHE_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::cache_dir)
        .expect("Failed to get cache directory");
    let expected_cache_dir = expected_base.join("nu-agent").join("sessions");

    // Act: Create SessionStore without XDG override (uses env var if present)
    let store = SessionStore::new_with_xdg_override(None);

    // Assert: Should use default cache directory (XDG_CACHE_HOME or platform default)
    assert_eq!(
        store.cache_dir(),
        &expected_cache_dir,
        "Should use default cache directory"
    );
    assert!(
        expected_cache_dir.exists(),
        "Default cache directory should be created"
    );

    // Cleanup
    let _ = fs::remove_dir_all(expected_base.join("nu-agent"));
}

#[test]
fn test_session_store_reuses_existing_directory() {
    // Arrange: Create a cache directory manually
    let temp_dir = env::temp_dir().join("nu-agent-test-reuse");
    let cache_dir = temp_dir.join("sessions");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");

    // Create a test file in the directory
    let test_file = cache_dir.join("test.txt");
    fs::write(&test_file, "test content").expect("Failed to write test file");

    // Act: Create SessionStore with existing directory
    let store = SessionStore::new_with_cache_dir(cache_dir.clone());

    // Assert: Directory and existing file should still exist
    assert!(cache_dir.exists(), "Cache directory should still exist");
    assert!(test_file.exists(), "Existing file should not be deleted");
    assert_eq!(store.cache_dir(), &cache_dir);

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_resolve_cache_dir_with_xdg_override() {
    // Arrange: Create override path
    let override_path = PathBuf::from("/tmp/custom-cache");

    // Act: Resolve cache directory with override
    let result = SessionStore::resolve_cache_dir(Some(override_path.clone()));

    // Assert: Should use override path with nu-agent/sessions appended
    assert_eq!(result, override_path.join("nu-agent").join("sessions"));
}

#[test]
fn test_resolve_cache_dir_uses_default_when_no_override() {
    // Act: Resolve cache directory without override
    let result = SessionStore::resolve_cache_dir(None);

    // Assert: Should contain nu-agent/sessions path
    assert!(
        result.to_string_lossy().contains("nu-agent"),
        "Path should contain 'nu-agent'"
    );
    assert!(
        result.to_string_lossy().ends_with("sessions"),
        "Path should end with 'sessions'"
    );
}
