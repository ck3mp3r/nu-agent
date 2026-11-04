//! Shared fixtures for the architecture drivers.

use std::fs;
use std::path::{Path, PathBuf};

pub type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// The assertion ARCH-12, the kernel's dependency boundary.
pub const ARCH_12: &str = "ARCH-12";

/// The recorded baselines, one row per assertion.
///
/// An empty row is a property that holds everywhere in today's tree. A row that
/// is not empty is a counted defect: the assertion passes while its set of
/// violations equals the row exactly.
pub const BASELINES: [(&str, &[&str]); 1] = [(ARCH_12, &[])];

/// The scratch directory fixture trees are built in, relative to this crate.
const FIXTURE_ROOT: &str = "../../tmp/architecture";

// endregion: --- Fixtures

// region:    --- Support

/// The baseline recorded for `assertion`, empty when none is.
pub fn baseline(assertion: &str) -> &'static [&'static str] {
    BASELINES
        .iter()
        .find(|(id, _)| *id == assertion)
        .map_or(&[], |(_, entries)| *entries)
}

/// An empty fixture workspace root named `name`, created fresh.
pub fn fixture_root(name: &str) -> Result<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join(name);
    if root.is_dir() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    Ok(root)
}

/// Write `text` as the manifest of `krate` in the fixture workspace `root`.
pub fn write_manifest(root: &Path, krate: &str, text: &str) -> Result<()> {
    let dir = root.join("crates").join(krate);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("Cargo.toml"), text)?;
    Ok(())
}

// endregion: --- Support
