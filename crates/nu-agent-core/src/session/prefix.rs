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
                char::from_digit(hi, 16).expect("hex digit"),
                char::from_digit(lo, 16).expect("hex digit"),
            ]
        })
        .take(7)
        .collect()
}
