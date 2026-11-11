//! ARCH-12: the kernel's dependency boundary.

use crate::architecture::report::Violation;
use crate::architecture::sources::Workspace;

// region:    --- Functions

/// WHEN the workspace's manifests are read, THE `agnt-kernel` crate SHALL list
/// no workspace crate in `[dependencies]` or `[build-dependencies]`.
///
/// A `[dev-dependencies]` entry is not a violation: the kernel's boundary is
/// about what production carries, and a test may read whatever it needs.
pub fn arch_12_kernel_dependencies(ws: &Workspace) -> Vec<Violation> {
    let manifest = ws.manifest(KERNEL);
    let mut violations = Vec::new();
    for (table, keys) in [
        (DEPENDENCIES, manifest.dependencies()),
        (BUILD_DEPENDENCIES, manifest.build_dependencies()),
    ] {
        violations.extend(keys.iter().filter(|(key, _)| is_workspace_crate(key)).map(
            |(key, line)| Violation {
                assertion: ASSERTION,
                site: format!("{KERNEL}/{MANIFEST_FILE}:{line}"),
                detail: format!("`{table}` lists `{key}`"),
            },
        ));
    }
    violations
}

// endregion: --- Functions

// region:    --- Support

/// The assertion this module reports.
const ASSERTION: &str = "ARCH-12";

/// The crate whose dependency boundary the assertion pins.
const KERNEL: &str = "agnt-kernel";

/// The manifest file name, relative to the crate directory.
const MANIFEST_FILE: &str = "Cargo.toml";

const DEPENDENCIES: &str = "dependencies";
const BUILD_DEPENDENCIES: &str = "build-dependencies";

/// The prefix every workspace crate carries.
const WORKSPACE_CRATE: &str = "agnt-";

/// Whether `key` names a crate of this workspace.
fn is_workspace_crate(key: &str) -> bool {
    key.starts_with(WORKSPACE_CRATE)
}

// endregion: --- Support
