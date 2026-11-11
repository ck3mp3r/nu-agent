//! ARCH-12: `agnt-kernel` carries no workspace dependency in production.

use crate::architecture::{
    Violation, Workspace, arch_12_kernel_dependencies, assert_matches_baseline,
};

use super::support::{self, ARCH_12, Result};

// region:    --- Fixtures

/// The crate the assertion pins.
const KERNEL: &str = "agnt-kernel";

/// A fixture manifest whose `[dependencies]` names a workspace crate.
const ROGUE_DEPENDENCIES: &str = concat!(
    "[package]\n",
    "name = \"agnt-kernel\"\n",
    "\n",
    "[dependencies]\n",
    "agnt-kernel = { workspace = true }\n",
);

/// The same dependency, under `[dev-dependencies]`.
const ROGUE_DEV_DEPENDENCIES: &str = concat!(
    "[package]\n",
    "name = \"agnt-kernel\"\n",
    "\n",
    "[dev-dependencies]\n",
    "agnt-kernel = { workspace = true }\n",
);

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the check runs against today's tree, THE violation set SHALL be empty
/// and SHALL equal the recorded baseline.
#[test]
fn test_graph_arch_12_kernel_dependencies_pass_on_empty_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;
    let manifest = ws.manifest(KERNEL);
    let declared: Vec<&str> = manifest
        .dependencies()
        .iter()
        .map(|(key, _)| key.as_str())
        .collect();

    // -- Exec
    let found = arch_12_kernel_dependencies(&ws);

    // -- Check
    assert!(
        declared.contains(&"serde"),
        "the kernel's manifest must be loaded, not defaulted: {declared:?}"
    );
    assert!(found.is_empty(), "{found:#?}");
    assert_matches_baseline(ARCH_12, &found, support::baseline(ARCH_12));
    Ok(())
}

/// WHEN a manifest lists a workspace crate under `[dependencies]`, THE check
/// SHALL report it at the line it sits on.
#[test]
fn test_graph_arch_12_kernel_dependencies_flags_a_workspace_dependency() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-12-dependencies")?;
    support::write_manifest(&root, KERNEL, ROGUE_DEPENDENCIES)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_12_kernel_dependencies(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_12);
    assert_eq!(found[0].site, "agnt-kernel/Cargo.toml:5");
    assert!(found[0].detail.contains("agnt-kernel"), "{:#?}", found[0]);
    Ok(())
}

/// WHEN a manifest lists a workspace crate under `[dev-dependencies]`, THE
/// check SHALL report nothing: a test may read what production may not.
#[test]
fn test_graph_arch_12_kernel_dependencies_ignores_a_dev_dependency() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-12-dev-dependencies")?;
    support::write_manifest(&root, KERNEL, ROGUE_DEV_DEPENDENCIES)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found: Vec<Violation> = arch_12_kernel_dependencies(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers
