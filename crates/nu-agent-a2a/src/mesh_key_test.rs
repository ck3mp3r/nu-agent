use std::path::Path;

use crate::mesh_key;

const TEST_CWD: &str = "/some/test/path";

fn test_path() -> &'static Path {
    Path::new(TEST_CWD)
}

#[test]
fn test_default_mesh_key_is_deterministic() {
    let a = mesh_key::default_mesh_key(test_path());
    let b = mesh_key::default_mesh_key(test_path());
    assert_eq!(a, b);
}

#[test]
fn test_default_mesh_key_is_7_chars() {
    let key = mesh_key::default_mesh_key(test_path());
    assert_eq!(key.len(), 7);
}

#[test]
fn test_default_mesh_key_differs_for_diff_paths() {
    let a = mesh_key::default_mesh_key(Path::new("/project-a"));
    let b = mesh_key::default_mesh_key(Path::new("/project-b"));
    assert_ne!(a, b, "different paths must produce different hashes");
}

#[test]
fn test_explicit_key_wins() {
    let result = mesh_key::resolve_mesh_key(Some("explicit".to_string()), test_path());
    assert_eq!(result, "explicit");
}

#[test]
fn test_env_var_wins() {
    let old = std::env::var("AGENT_MESH_KEY").ok();
    unsafe {
        std::env::set_var("AGENT_MESH_KEY", "env-key");
    }

    let result = mesh_key::resolve_mesh_key(None, test_path());
    assert_eq!(result, "env-key");

    unsafe {
        match old {
            Some(val) => std::env::set_var("AGENT_MESH_KEY", val),
            None => std::env::remove_var("AGENT_MESH_KEY"),
        }
    }
}

#[test]
fn test_default_falls_back_to_cwd_hash() {
    let result = mesh_key::resolve_mesh_key(None, test_path());
    assert_eq!(result, mesh_key::default_mesh_key(test_path()));
}
