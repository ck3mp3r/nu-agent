use super::prefix::{dir_prefix, dir_prefix_legacy, filter_sessions_by_cwd};
use crate::session::SessionInfo;
use chrono::Utc;
use std::path::Path;

#[test]
fn dir_prefix_returns_16_hex_chars() {
    let result = dir_prefix(Path::new("/home/user/project"));
    assert_eq!(result.len(), 16);
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
    // SHA-256 of "/home/user/project" -> first 16 hex chars must be "9dad1e4e08b0b11c"
    // If this test breaks, the hashing algorithm has changed -- update all stored session IDs.
    assert_eq!(
        dir_prefix(Path::new("/home/user/project")),
        "9dad1e4e08b0b11c"
    );
}

#[test]
fn dir_prefix_legacy_returns_7_hex_chars() {
    let result = dir_prefix_legacy(Path::new("/home/user/project"));
    assert_eq!(result.len(), 7);
    assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn dir_prefix_legacy_pinned_value() {
    // SHA-256 of "/home/user/project" -> first 7 hex chars must be "9dad1e4"
    assert_eq!(
        dir_prefix_legacy(Path::new("/home/user/project")),
        "9dad1e4"
    );
}

#[test]
fn dir_prefix_is_superset_of_legacy() {
    let path = Path::new("/home/user/project");
    assert!(dir_prefix(path).starts_with(&dir_prefix_legacy(path)));
}

fn session(id: &str) -> SessionInfo {
    SessionInfo {
        id: id.to_string(),
        message_count: 0,
        last_active: Utc::now(),
        title: None,
    }
}

#[test]
fn filter_sessions_by_cwd_returns_only_matching_prefixes() {
    let cwd = Path::new("/home/user/project");
    let new = dir_prefix(cwd);
    let legacy = dir_prefix_legacy(cwd);
    let unrelated = dir_prefix(Path::new("/other/dir"));
    let sessions = vec![
        session(&format!("{new}-abc")),
        session(&format!("{legacy}-def")),
        session(&format!("{unrelated}-ghi")),
    ];
    let result = filter_sessions_by_cwd(sessions, cwd);
    assert_eq!(result.len(), 2);
    assert!(result.iter().any(|s| s.id == format!("{new}-abc")));
    assert!(result.iter().any(|s| s.id == format!("{legacy}-def")));
}

#[test]
fn filter_sessions_by_cwd_empty_input_returns_empty() {
    let result = filter_sessions_by_cwd(Vec::new(), Path::new("/home/user/project"));
    assert!(result.is_empty());
}

#[test]
fn filter_sessions_by_cwd_all_match_returns_all() {
    let cwd = Path::new("/home/user/project");
    let new = dir_prefix(cwd);
    let sessions = vec![
        session(&format!("{new}-abc")),
        session(&format!("{new}-def")),
    ];
    let result = filter_sessions_by_cwd(sessions, cwd);
    assert_eq!(result.len(), 2);
}

#[test]
fn filter_sessions_by_cwd_no_match_returns_empty() {
    let cwd = Path::new("/home/user/project");
    let unrelated = dir_prefix(Path::new("/other/dir"));
    let sessions = vec![session(&format!("{unrelated}-abc"))];
    let result = filter_sessions_by_cwd(sessions, cwd);
    assert!(result.is_empty());
}
