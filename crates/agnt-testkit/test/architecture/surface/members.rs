//! The impl-member drivers: what a `pub` member needs to have a consumer.
//!
//! A member is reachable through a path or through a receiver call, so the member
//! half reads both. The receiver call is gated: it credits a member only from a
//! crate that also writes a path into the publishing crate, because the call
//! itself names no crate.

use super::super::support;
use super::*;

// region:    --- Fixtures

/// A `pub` type with a `pub` member nothing outside its crate reaches.
const MEMBER_LONELY: &str =
    "pub struct Thing;\n\nimpl Thing {\n    pub fn solo() -> usize {\n        0\n    }\n}\n";

/// The member reached by another crate through the crate path.
const MEMBER_CONSUMER_SOURCE: &str = "fn call() -> usize {\n    agnt_runtime::Thing::solo()\n}\n";

/// A `pub` member on a type the crate does not surface.
const MEMBER_HIDDEN_TYPE: &str =
    "struct Hidden;\n\nimpl Hidden {\n    pub fn solo() -> usize {\n        0\n    }\n}\n";

/// A `pub` type whose associated function nothing outside its crate reaches.
const MINT_LONELY: &str =
    "pub struct Thing;\n\nimpl Thing {\n    pub fn mint() -> Self {\n        Thing\n    }\n}\n";

/// A consumer that imports the type by name and calls its associated function:
/// no crate segment appears in the call, so only the import attributes it.
const IMPORTED_CALL_SOURCE: &str =
    "use agnt_runtime::Thing;\n\nfn call() -> Thing {\n    Thing::mint()\n}\n";

/// A consumer that imports the type and never calls its associated function.
const IMPORT_ONLY_SOURCE: &str = "use agnt_runtime::Thing;\n\nfn call(_: Thing) {}\n";

/// A consumer that calls an associated function of the same name on its own,
/// never-imported type.
const LOCAL_CALL_SOURCE: &str = "struct Thing;\n\nimpl Thing {\n    fn mint() -> Self {\n        Thing\n    }\n}\n\nfn call() -> Thing {\n    Thing::mint()\n}\n";

/// A consumer that calls the member through a receiver and writes no path into
/// the publishing crate.
const MEMBER_RECEIVER_SOURCE: &str = "fn call(thing: &Thing) -> usize {\n    thing.solo()\n}\n";

/// A consumer that references the publishing crate and calls the member through
/// a receiver: the import is the reference the gate reads.
const MEMBER_RECEIVER_REFERENCED_SOURCE: &str =
    "use agnt_runtime::Thing;\n\nfn call(thing: &Thing) -> usize {\n    thing.solo()\n}\n";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN a `pub` member on a surfaced type has no consumer outside its crate,
/// THEN the member census SHALL report its declaration site.
#[test]
fn test_surface_arch_6_impl_member_census_flags_an_unconsumed_member() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-member-lonely")?;
    support::write_source(&root, PUBLISHER, SOURCE, MEMBER_LONELY)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_impl_member_census(&ws);

    // -- Check
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].assertion, ARCH_6);
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:4");
    assert_eq!(
        found[0].detail,
        "`solo` has no consumer outside `agnt-runtime`"
    );
    Ok(())
}

/// WHEN another crate reaches the member through the crate path, THEN the
/// member census SHALL report nothing: that reach is the evidence a re-record
/// removes the row on.
#[test]
fn test_surface_arch_6_impl_member_census_suppresses_a_consumed_member() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-member-consumed")?;
    support::write_source(&root, PUBLISHER, SOURCE, MEMBER_LONELY)?;
    support::write_source(&root, CONSUMER, SOURCE, MEMBER_CONSUMER_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_impl_member_census(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN the member's `impl` attaches to a type the crate does not surface,
/// THEN the member census SHALL report nothing: only surfaced types carry census
/// surface.
#[test]
fn test_surface_arch_6_impl_member_census_ignores_an_unsurfaced_type() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-member-hidden-type")?;
    support::write_source(&root, PUBLISHER, SOURCE, MEMBER_HIDDEN_TYPE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_impl_member_census(&ws);

    // -- Check
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

/// WHEN another crate imports the type by name and calls its associated function
/// through that name, THEN the member census SHALL count the call as a consumer:
/// `Thing::mint()` carries no crate segment and is no receiver call.
#[test]
fn test_surface_arch_6_impl_member_census_counts_an_imported_path_call() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-member-imported-call")?;
    support::write_source(&root, PUBLISHER, SOURCE, MINT_LONELY)?;
    support::write_source(&root, CONSUMER, SOURCE, IMPORTED_CALL_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_impl_member_census(&ws);

    // -- Check
    assert!(
        found.is_empty(),
        "the imported call consumes `mint`: {found:#?}"
    );
    Ok(())
}

/// WHEN another crate imports the type and never calls the member, THEN the row
/// SHALL stay: the rule counts a call, not an import.
#[test]
fn test_surface_arch_6_impl_member_census_keeps_a_member_only_imported() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-member-import-only")?;
    support::write_source(&root, PUBLISHER, SOURCE, MINT_LONELY)?;
    support::write_source(&root, CONSUMER, SOURCE, IMPORT_ONLY_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_impl_member_census(&ws);

    // -- Check
    assert_eq!(
        found.len(),
        1,
        "an import without a call is not evidence: {found:#?}"
    );
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:4");
    Ok(())
}

/// WHEN another crate calls `Thing::mint()` on a type of its own of that name,
/// THEN the row SHALL stay: a leading name the file never imported attributes
/// nothing, so a same-named local call is not another crate's consumer.
#[test]
fn test_surface_arch_6_impl_member_census_keeps_a_call_on_a_local_type() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-member-local-call")?;
    support::write_source(&root, PUBLISHER, SOURCE, MINT_LONELY)?;
    support::write_source(&root, CONSUMER, SOURCE, LOCAL_CALL_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_impl_member_census(&ws);

    // -- Check
    assert_eq!(
        found.len(),
        1,
        "a local `Thing::mint()` consumes nothing: {found:#?}"
    );
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:4");
    Ok(())
}

/// WHEN a crate that names no path into the publishing crate calls the member
/// through a receiver, THEN the member census SHALL keep the row: a receiver
/// call credits a member only across a reference.
#[test]
fn test_surface_arch_6_impl_member_census_keeps_a_receiver_call_without_a_reference() -> Result<()>
{
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-member-receiver-unreferenced")?;
    support::write_source(&root, PUBLISHER, SOURCE, MEMBER_LONELY)?;
    support::write_source(&root, CONSUMER, SOURCE, MEMBER_RECEIVER_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_impl_member_census(&ws);

    // -- Check
    assert_eq!(
        found.len(),
        1,
        "a receiver call from an unreferencing crate is no evidence: {found:#?}"
    );
    assert_eq!(found[0].site, "agnt-runtime/src/mod.rs:4");
    Ok(())
}

/// WHEN a crate that references the publishing crate calls the member through a
/// receiver, THEN the member census SHALL credit it: the reference is what makes
/// the receiver call admissible.
#[test]
fn test_surface_arch_6_impl_member_census_counts_a_receiver_call_with_a_reference() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-member-receiver-referenced")?;
    support::write_source(&root, PUBLISHER, SOURCE, MEMBER_LONELY)?;
    support::write_source(&root, CONSUMER, SOURCE, MEMBER_RECEIVER_REFERENCED_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_impl_member_census(&ws);

    // -- Check
    assert!(
        found.is_empty(),
        "a referencing crate's receiver call consumes `solo`: {found:#?}"
    );
    Ok(())
}

// endregion: --- Drivers
