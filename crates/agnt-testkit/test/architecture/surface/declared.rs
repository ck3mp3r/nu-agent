//! The declared-item drivers: what a free item needs to have a consumer.
//!
//! A free item is reachable through a path alone — a crate-rooted path, a
//! `use`-tree leaf, or a call whose callee starts with a name the file imported.
//! A receiver call names a method, so it consumes no declared item.

use super::super::support;
use super::*;

// region:    --- Fixtures

/// A public item nothing outside its crate reaches.
const LONELY: &str = "pub fn lonely() -> usize {\n    0\n}\n";

/// A public item the publishing crate only uses itself.
const SELF_USED: &str =
    "pub fn lonely() -> usize {\n    0\n}\n\nfn call() -> usize {\n    lonely()\n}\n";

/// The same item, reached by another crate through the crate path.
const CONSUMER_SOURCE: &str = "fn call() -> usize {\n    agnt_runtime::lonely()\n}\n";

/// A consumer that names the item only through a method receiver: the call
/// carries the method name alone, and no crate path appears in it.
const METHOD_CALL_SOURCE: &str = "fn call(rig: &Rig) -> usize {\n    rig.lonely()\n}\n";

/// A consumer that reads a field of the same name and never calls it.
const FIELD_ONLY_SOURCE: &str = "fn call(rig: &Rig) -> usize {\n    rig.lonely\n}\n";

/// The default binary target of a package: `src/main.rs`.
const BIN_TARGET: &str = "main.rs";

/// A named binary target of a package: a file under `src/bin/`.
const NAMED_BIN_TARGET: &str = "bin/tool.rs";

/// A further file of a package's library target, beside its root module.
const LIB_SIBLING: &str = "other.rs";

/// The publisher calling its own item through a receiver: same crate, so it is
/// not evidence of a consumer anywhere else.
const SELF_CALL_SOURCE: &str =
    "pub fn lonely() -> usize {\n    0\n}\n\nfn call(rig: &Rig) -> usize {\n    rig.lonely()\n}\n";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN a crate publishes an item no other crate reaches, THEN the assertion
/// SHALL report its declaration site.
#[test]
fn test_surface_arch_6_public_surface_census_flags_a_consumer_less_item() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-consumer-less")?;
    support::write_source(&root, PUBLISHER, SOURCE, LONELY)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_6);
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:1");
    Ok(())
}

/// WHEN another crate reaches the item through the crate path, THEN the
/// assertion SHALL report nothing: a cross-crate path is the consumer evidence.
#[test]
fn test_surface_arch_6_public_surface_census_suppresses_a_consumed_item() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-consumed")?;
    support::write_source(&root, PUBLISHER, SOURCE, SELF_USED)?;
    support::write_source(&root, CONSUMER, SOURCE, CONSUMER_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN a package's binary target reaches a `pub` item of its own library, THEN
/// the census SHALL credit the library row: `src/main.rs` compiles into a target
/// that names the library as a dependency, so the path crosses a crate boundary.
#[test]
fn test_surface_arch_6_public_surface_census_counts_a_bin_target_reference() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-bin-target")?;
    support::write_source(&root, PUBLISHER, SOURCE, LONELY)?;
    support::write_source(&root, PUBLISHER, BIN_TARGET, CONSUMER_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert!(
        found.is_empty(),
        "the package's bin consumes `lonely`: {found:#?}"
    );
    Ok(())
}

/// WHEN a package's binary sits under `src/bin/`, THEN the census SHALL credit
/// the library row too: the key reads the target, not the file name `main.rs`.
#[test]
fn test_surface_arch_6_public_surface_census_counts_a_named_bin_target_reference() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-named-bin-target")?;
    support::write_source(&root, PUBLISHER, SOURCE, LONELY)?;
    support::write_source(&root, PUBLISHER, NAMED_BIN_TARGET, CONSUMER_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert!(
        found.is_empty(),
        "a named bin consumes `lonely`: {found:#?}"
    );
    Ok(())
}

/// WHEN a library file of the same package reaches the item, THEN the row SHALL
/// stay: the library target is the crate the row is about, so its own paths are
/// no evidence of a consumer, and the bin-target key must not loosen that.
#[test]
fn test_surface_arch_6_public_surface_census_keeps_a_lib_sibling_reference() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-lib-sibling")?;
    support::write_source(&root, PUBLISHER, SOURCE, LONELY)?;
    support::write_source(&root, PUBLISHER, LIB_SIBLING, CONSUMER_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert_eq!(
        found.len(),
        1,
        "a lib file's own path consumes nothing: {found:#?}"
    );
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:1");
    Ok(())
}

/// WHEN the only use is inside the declaring crate, THEN the row SHALL stay:
/// the property is about surface across a crate boundary.
#[test]
fn test_surface_arch_6_public_surface_census_keeps_a_self_used_item() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-self-used")?;
    support::write_source(&root, PUBLISHER, SOURCE, SELF_USED)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:1");
    Ok(())
}

/// WHEN another crate calls the name through a method receiver, THEN the census
/// SHALL keep the row: a receiver call records a method name, and no declared
/// item is reachable through a receiver.
#[test]
fn test_surface_arch_6_public_surface_census_keeps_a_method_call_receiver() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-method-receiver")?;
    support::write_source(&root, PUBLISHER, SOURCE, LONELY)?;
    support::write_source(&root, CONSUMER, SOURCE, METHOD_CALL_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert_eq!(
        found.len(),
        1,
        "a receiver call consumes no free item: {found:#?}"
    );
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:1");
    Ok(())
}

/// WHEN another crate only reads a field of the same name, THEN the row SHALL
/// stay: a field access is not a call, and the census counts calls.
#[test]
fn test_surface_arch_6_public_surface_census_keeps_a_field_that_is_never_called() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-method-field-only")?;
    support::write_source(&root, PUBLISHER, SOURCE, LONELY)?;
    support::write_source(&root, CONSUMER, SOURCE, FIELD_ONLY_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "a field read is not a call: {found:#?}");
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:1");
    Ok(())
}

/// WHEN only the declaring crate calls the item through a receiver, THEN the row
/// SHALL stay: cross-crate is the law, and a self-call is not a consumer.
///
/// The two cross-crate tests cannot catch a self-include: they put a second
/// crate in the fixture, so a key mismatch would still see a real consumer. This
/// fixture has one crate only.
#[test]
fn test_surface_arch_6_public_surface_census_keeps_an_item_only_its_crate_calls() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-self-call")?;
    support::write_source(&root, PUBLISHER, SOURCE, SELF_CALL_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert_eq!(
        found.len(),
        1,
        "a self-call must not consume the row: {found:#?}"
    );
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:1");
    Ok(())
}

/// WHEN a workspace holds no sources, THEN the census SHALL report nothing and
/// the real driver's own non-empty guard SHALL be what stops an empty parse from
/// passing.
#[test]
fn test_surface_arch_6_public_surface_census_negative_on_an_empty_workspace() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-empty")?;
    support::write_manifest(&root, "empty-crate", "[package]\nname = \"empty-crate\"\n")?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    assert!(
        ws.sources().is_empty(),
        "the negative fixture must hold no sources, or it proves nothing"
    );
    Ok(())
}

// endregion: --- Drivers
