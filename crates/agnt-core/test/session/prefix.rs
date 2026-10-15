use super::prefix::{dir_prefix, dir_prefix_legacy, filter_sessions_by_cwd};
use crate::session::SessionInfo;
use chrono::Utc;
use std::path::Path;

/// `dir_prefix` is a deterministic 16-hex-char digest of the cwd.
#[test]
fn dir_prefix_shape_and_determinism() {
    // -- Case 1: 16 hex chars
    let result = dir_prefix(Path::new("/home/user/project"));
    assert_eq!(result.len(), 16);
    assert!(result.chars().all(|c| c.is_ascii_hexdigit()));

    // -- Case 2: deterministic for the same path
    assert_eq!(
        dir_prefix(Path::new("/home/user/project")),
        dir_prefix(Path::new("/home/user/project"))
    );

    // -- Case 3: different paths produce different prefixes
    assert_ne!(
        dir_prefix(Path::new("/home/user/project-a")),
        dir_prefix(Path::new("/home/user/project-b"))
    );

    // -- Case 4: pinned value
    // SHA-256 of "/home/user/project" -> first 16 hex chars must be "9dad1e4e08b0b11c".
    // If this breaks, the hashing algorithm has changed -- update all stored session IDs.
    assert_eq!(
        dir_prefix(Path::new("/home/user/project")),
        "9dad1e4e08b0b11c"
    );
}

/// `dir_prefix_legacy` is the 7-hex-char prefix used before the length
/// increase, and is a prefix of the current value.
#[test]
fn dir_prefix_legacy_shape_and_pinned_value() {
    // -- Case 1: 7 hex chars
    let result = dir_prefix_legacy(Path::new("/home/user/project"));
    assert_eq!(result.len(), 7);
    assert!(result.chars().all(|c| c.is_ascii_hexdigit()));

    // -- Case 2: pinned value
    // SHA-256 of "/home/user/project" -> first 7 hex chars must be "9dad1e4".
    assert_eq!(
        dir_prefix_legacy(Path::new("/home/user/project")),
        "9dad1e4"
    );

    // -- Case 3: the current prefix is a superset of the legacy prefix
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

/// `filter_sessions_by_cwd` keeps only sessions whose id carries the current
/// or legacy cwd prefix.
#[test]
fn filter_sessions_by_cwd_keeps_matching_prefixes() {
    // -- Case 1: only matching prefixes are kept
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

    // -- Case 2: all matching sessions are returned
    let sessions = vec![
        session(&format!("{new}-abc")),
        session(&format!("{new}-def")),
    ];
    assert_eq!(filter_sessions_by_cwd(sessions, cwd).len(), 2);

    // -- Case 3: no match returns empty
    let sessions = vec![session(&format!("{unrelated}-abc"))];
    assert!(filter_sessions_by_cwd(sessions, cwd).is_empty());

    // -- Case 4: empty input returns empty
    assert!(filter_sessions_by_cwd(Vec::new(), cwd).is_empty());
}
