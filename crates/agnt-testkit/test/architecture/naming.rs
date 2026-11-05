//! ARCH-10: no consumer names the aggregate runtime type.

use crate::architecture::{
    Workspace, arch_10_aggregate_runtime_references, assert_matches_baseline,
};

use super::support::{self, ARCH_10, Result};

// region:    --- Fixtures

/// The crate every fixture lives in: a consumer crate.
const CRATE: &str = "agnt-cli";

/// A file with no recorded references.
const SOURCE: &str = "rogue.rs";

/// A signature that names the aggregate.
const NAMED_IN_SIGNATURE: &str = "pub fn f(_runtime: &AgentConversationRuntime) {}\n";

/// The same name in a doc comment: a string literal, never a path.
const NAMED_IN_DOC: &str = "/// `AgentConversationRuntime` drives a mode.\npub fn f() {}\n";

/// A different type whose name starts with the aggregate's.
const BUILDER: &str = "pub fn f(_b: AgentRuntimeBuilder) {}\n";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the census runs against today's tree, THE violation set SHALL equal
/// the recorded baseline exactly.
#[test]
fn test_naming_arch_10_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;
    let loaded = ws
        .sources()
        .iter()
        .any(|(label, _)| label == "agnt-cli/src/mode/tui.rs");

    // -- Exec
    let found = arch_10_aggregate_runtime_references(&ws);

    // -- Check
    assert!(loaded, "the consumer crates must be loaded");
    assert_matches_baseline(ARCH_10, &found, support::baseline(ARCH_10));
    Ok(())
}

/// WHEN a consumer's signature names the aggregate, THEN the assertion SHALL
/// report that line.
#[test]
fn test_naming_arch_10_flags_a_named_signature() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-10-named-signature")?;
    support::write_source(&root, CRATE, SOURCE, NAMED_IN_SIGNATURE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_10_aggregate_runtime_references(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_10);
    assert_eq!(found[0].site, "agnt-cli/src/rogue.rs:1");
    assert_eq!(found[0].detail, "AgentConversationRuntime");
    Ok(())
}

/// WHEN a doc comment names the aggregate, THEN the assertion SHALL report
/// nothing: a name in text is not a reference.
#[test]
fn test_naming_arch_10_ignores_a_doc_comment() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-10-named-in-doc")?;
    support::write_source(&root, CRATE, SOURCE, NAMED_IN_DOC)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_10_aggregate_runtime_references(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a different type carries the aggregate's name as a prefix, THEN the
/// assertion SHALL report nothing: the match is the last segment, exactly.
#[test]
fn test_naming_arch_10_ignores_a_prefixed_type() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-10-prefixed-type")?;
    support::write_source(&root, CRATE, SOURCE, BUILDER)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_10_aggregate_runtime_references(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

// endregion: --- Drivers
