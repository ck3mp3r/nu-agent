use super::prefix::dir_prefix;
use std::path::Path;

#[test]
fn dir_prefix_returns_7_hex_chars() {
    let result = dir_prefix(Path::new("/home/user/project"));
    assert_eq!(result.len(), 7);
    assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn dir_prefix_is_deterministic() {
    let a = dir_prefix(Path::new("/home/user/project"));
    let b = dir_prefix(Path::new("/home/user/project"));
    assert_eq!(a, b);
}

#[test]
fn dir_prefix_differs_for_different_paths() {
    let a = dir_prefix(Path::new("/home/user/project-a"));
    let b = dir_prefix(Path::new("/home/user/project-b"));
    assert_ne!(a, b);
}

#[test]
fn dir_prefix_pinned_value() {
    // SHA-256 of "/home/user/project" -> first 7 hex chars must be "9dad1e4"
    // If this test breaks, the hashing algorithm has changed -- update all stored session IDs.
    assert_eq!(dir_prefix(Path::new("/home/user/project")), "9dad1e4");
}
