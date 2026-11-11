//! ARCH-9.a: the aggregate runtime exposes no publicly settable field.

use crate::architecture::{Workspace, arch_9a_public_runtime_fields, assert_matches_baseline};

use super::support::{self, ARCH_9A, Result};

// region:    --- Fixtures

/// The crate the rogue fixture lives in.
const CRATE: &str = "agnt-runtime";

/// The source every rogue fixture writes, inside the aggregate's directory.
const SOURCE: &str = "conversation/runtime.rs";

/// A `pub struct` with one `pub` field and one private one.
const PUB_FIELD: &str = concat!(
    "pub struct AgentRuntime {\n",
    "    pub bus: u8,\n",
    "    hidden: u8,\n",
    "}\n",
);

/// The same field on a private struct: the aggregate's surface is the type's.
const PRIVATE_STRUCT: &str = concat!("struct AgentRuntime {\n", "    pub bus: u8,\n", "}\n",);

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the assertion runs against today's tree, THE violation set SHALL equal
/// the recorded baseline exactly.
#[test]
fn test_composition_arch_9a_public_runtime_fields_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;
    let loaded = ws
        .sources()
        .iter()
        .any(|(label, _)| label.starts_with("agnt-runtime/src/conversation/"));

    // -- Exec
    let found = arch_9a_public_runtime_fields(&ws);

    // -- Check
    assert!(
        loaded,
        "the aggregate's sources must be loaded, or an empty set proves nothing"
    );
    assert_matches_baseline(ARCH_9A, &found, support::baseline(ARCH_9A));
    Ok(())
}

/// WHEN a `pub struct` in the aggregate's directory declares a `pub` field,
/// THEN the assertion SHALL report the field's line.
#[test]
fn test_composition_arch_9a_flags_a_public_field() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-9a-public-field")?;
    support::write_source(&root, CRATE, SOURCE, PUB_FIELD)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_9a_public_runtime_fields(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_9A);
    assert_eq!(found[0].site, "agnt-runtime/src/conversation/runtime.rs:2");
    assert_eq!(found[0].detail, "pub bus");
    Ok(())
}

/// WHEN the same field sits on a private struct, THEN the assertion SHALL
/// report nothing: the aggregate's public surface is what breaks the contract.
#[test]
fn test_composition_arch_9a_ignores_a_private_struct() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-9a-private-struct")?;
    support::write_source(&root, CRATE, SOURCE, PRIVATE_STRUCT)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_9a_public_runtime_fields(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers
