use super::prefix::{dir_prefix, filter_sessions_by_cwd};
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

fn session(id: &str) -> SessionInfo {
    SessionInfo {
        id: id.to_string(),
        message_count: 0,
        last_active: Utc::now(),
        title: None,
    }
}

/// `filter_sessions_by_cwd` keeps only sessions whose id carries the cwd
/// prefix.
#[test]
fn filter_sessions_by_cwd_keeps_matching_prefixes() {
    // -- Case 1: only matching prefixes are kept
    let cwd = Path::new("/home/user/project");
    let matching = dir_prefix(cwd);
    let unrelated = dir_prefix(Path::new("/other/dir"));
    let sessions = vec![
        session(&format!("{matching}-abc")),
        session(&format!("{unrelated}-ghi")),
    ];
    let result = filter_sessions_by_cwd(sessions, cwd);
    assert_eq!(result.len(), 1);
    assert!(result.iter().any(|s| s.id == format!("{matching}-abc")));

    // -- Case 2: all matching sessions are returned
    let sessions = vec![
        session(&format!("{matching}-abc")),
        session(&format!("{matching}-def")),
    ];
    assert_eq!(filter_sessions_by_cwd(sessions, cwd).len(), 2);

    // -- Case 3: no match returns empty
    let sessions = vec![session(&format!("{unrelated}-abc"))];
    assert!(filter_sessions_by_cwd(sessions, cwd).is_empty());

    // -- Case 4: empty input returns empty
    assert!(filter_sessions_by_cwd(Vec::new(), cwd).is_empty());
}
