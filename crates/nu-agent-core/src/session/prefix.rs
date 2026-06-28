use sha2::{Digest, Sha256};
use std::path::Path;

pub fn dir_prefix(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    hash.iter()
        .flat_map(|b| {
            let hi = (b >> 4) as u32;
            let lo = (b & 0xf) as u32;
            [
                char::from_digit(hi, 16).unwrap(),
                char::from_digit(lo, 16).unwrap(),
            ]
        })
        .take(7)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // SHA-256 of "/home/user/project" → first 7 hex chars must be "9dad1e4"
        // If this test breaks, the hashing algorithm has changed — update all stored session IDs.
        assert_eq!(dir_prefix(Path::new("/home/user/project")), "9dad1e4");
    }
}
