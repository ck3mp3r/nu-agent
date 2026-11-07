use super::SessionInfo;
use sha2::{Digest, Sha256};
use std::path::Path;

fn hash_hex(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    hash.iter()
        .flat_map(|b| {
            let hi = (b >> 4) as u32;
            let lo = (b & 0xf) as u32;
            [
                char::from_digit(hi, 16).unwrap_or('0'),
                char::from_digit(lo, 16).unwrap_or('0'),
            ]
        })
        .collect()
}

/// 16-char hex prefix (64 bits of entropy) derived from the directory path.
pub fn dir_prefix(path: &Path) -> String {
    hash_hex(path).chars().take(16).collect()
}

/// Filters sessions to those whose ID starts with the prefix derived from
/// `cwd`. Used by both the CLI command and the TUI popup so they produce
/// identical results.
pub fn filter_sessions_by_cwd(sessions: Vec<SessionInfo>, cwd: &Path) -> Vec<SessionInfo> {
    let prefix = dir_prefix(cwd);
    sessions
        .into_iter()
        .filter(|info| info.id.starts_with(&prefix))
        .collect()
}
