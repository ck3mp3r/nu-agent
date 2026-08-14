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
                char::from_digit(hi, 16).expect("hex digit"),
                char::from_digit(lo, 16).expect("hex digit"),
            ]
        })
        .collect()
}

/// 16-char hex prefix (64 bits of entropy) derived from the directory path.
pub fn dir_prefix(path: &Path) -> String {
    hash_hex(path).chars().take(16).collect()
}

/// Legacy 7-char hex prefix used by sessions created before the prefix length
/// increase. Kept for backward-compatible lookups of existing stored sessions.
pub fn dir_prefix_legacy(path: &Path) -> String {
    hash_hex(path).chars().take(7).collect()
}

/// Returns the session ID with the matching `{prefix}-` portion stripped, or
/// `None` if neither the new nor the legacy prefix matches.
pub fn match_prefixs<'a>(id: &'a str, new: &str, legacy: &str) -> Option<&'a str> {
    id.strip_prefix(new)
        .and_then(|rest| rest.strip_prefix('-'))
        .or_else(|| {
            id.strip_prefix(legacy)
                .and_then(|rest| rest.strip_prefix('-'))
        })
}

/// Filters sessions to those whose ID starts with the new or legacy prefix
/// derived from `cwd`. Used by both the CLI command and the TUI popup so they
/// produce identical results.
pub fn filter_sessions_by_cwd(sessions: Vec<SessionInfo>, cwd: &Path) -> Vec<SessionInfo> {
    let new_prefix = dir_prefix(cwd);
    let legacy_prefix = dir_prefix_legacy(cwd);
    sessions
        .into_iter()
        .filter(|info| info.id.starts_with(&new_prefix) || info.id.starts_with(&legacy_prefix))
        .collect()
}
