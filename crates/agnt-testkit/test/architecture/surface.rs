//! ARCH-6: every declared public item has a consumer outside its crate.
//!
//! The recorded rows are a drain backlog, not compliance. The check is an
//! exact-set ratchet per crate: a fresh unconsumed item fails the build, and a
//! drained item's row leaves the baseline in the same commit.
//!
//! Both halves are exact sets. The declared-item rows live in `ROWS`, the
//! impl-member rows in `MEMBER_ROWS`, and the driver compares each half per crate
//! and asserts the partition is total, so a fresh unconsumed item or member fails
//! the build in the crate file that owes it.

use crate::architecture::{
    Violation, Workspace, arch_6_impl_member_census, arch_6_public_surface_consumer_census,
    assert_matches_baseline,
};

use super::support::{self, ARCH_6, ARCH_6_CRATES, ARCH_6_MEMBER_CRATES, Result};

// region:    --- Fixtures

/// The crate that publishes.
const PUBLISHER: &str = "agnt-runtime";

/// The crate that consumes.
const CONSUMER: &str = "agnt-tui";

/// The source both fixture crates write.
const SOURCE: &str = "mod.rs";

/// A public item nothing outside its crate reaches.
const LONELY: &str = "pub fn lonely() -> usize {\n    0\n}\n";

/// A public item the publishing crate only uses itself.
const SELF_USED: &str =
    "pub fn lonely() -> usize {\n    0\n}\n\nfn call() -> usize {\n    lonely()\n}\n";

/// The same item, reached by another crate through the crate path.
const CONSUMER_SOURCE: &str = "fn call() -> usize {\n    agnt_runtime::lonely()\n}\n";

/// A consumer that reaches the item only through a method receiver: the call
/// names the method, and no crate path appears in it.
const METHOD_CALL_SOURCE: &str = "fn call(rig: &Rig) -> usize {\n    rig.lonely()\n}\n";

/// A consumer that reads a field of the same name and never calls it.
const FIELD_ONLY_SOURCE: &str = "fn call(rig: &Rig) -> usize {\n    rig.lonely\n}\n";

/// The publisher calling its own item through a receiver: same crate, so it is
/// not evidence of a consumer anywhere else.
const SELF_CALL_SOURCE: &str =
    "pub fn lonely() -> usize {\n    0\n}\n\nfn call(rig: &Rig) -> usize {\n    rig.lonely()\n}\n";

/// A `pub` type with a `pub` member nothing outside its crate reaches.
const MEMBER_LONELY: &str =
    "pub struct Thing;\n\nimpl Thing {\n    pub fn solo() -> usize {\n        0\n    }\n}\n";

/// The member reached by another crate through the crate path.
const MEMBER_CONSUMER_SOURCE: &str = "fn call() -> usize {\n    agnt_runtime::Thing::solo()\n}\n";

/// A `pub` member on a type the crate does not surface.
const MEMBER_HIDDEN_TYPE: &str =
    "struct Hidden;\n\nimpl Hidden {\n    pub fn solo() -> usize {\n        0\n    }\n}\n";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN the census runs against today's tree, THE violation set SHALL equal the
/// recorded rows exactly — declared items and impl members alike — and THE
/// partition SHALL be total: every violation sits in one crate's record, and no
/// crate the walk reports on is unrecorded.
#[test]
fn test_surface_arch_6_public_surface_consumer_census_pass_on_baseline() -> Result<()> {
    // -- Setup & Fixtures
    let ws = Workspace::load()?;

    // -- Exec
    let items = arch_6_public_surface_consumer_census(&ws);
    let members = arch_6_impl_member_census(&ws);

    // -- Check
    assert!(
        !items.is_empty() && !members.is_empty(),
        "the census found no surface at all: an empty parse would pass this driver"
    );
    assert_eq!(
        ARCH_6_CRATES.len(),
        ARCH_6_MEMBER_CRATES.len(),
        "the two halves must record the same crates"
    );
    for ((krate, item_rows), (member_crate, member_rows)) in
        ARCH_6_CRATES.iter().zip(ARCH_6_MEMBER_CRATES.iter())
    {
        assert_eq!(krate, member_crate, "the two halves must be in crate order");
        let item_found = crate_rows(&items, krate);
        let member_found = crate_rows(&members, krate);
        assert_matches_baseline(ARCH_6, &item_found, item_rows);
        assert_matches_baseline(ARCH_6, &member_found, member_rows);
        assert_eq!(
            item_found.len() + member_found.len(),
            item_rows.len() + member_rows.len(),
            "{krate} rows must partition exactly"
        );
    }
    let recorded_items: usize = ARCH_6_CRATES.iter().map(|(_, rows)| rows.len()).sum();
    let recorded_members: usize = ARCH_6_MEMBER_CRATES
        .iter()
        .map(|(_, rows)| rows.len())
        .sum();
    assert_eq!(
        items.len() + members.len(),
        recorded_items + recorded_members,
        "every violation must sit in exactly one crate's baseline"
    );
    partition(&items)?;
    partition(&members)?;
    Ok(())
}

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

/// WHEN another crate calls the item through a method receiver, THEN the census
/// SHALL count that call as a consumer: `self.rig.open(id)` names `open` in a
/// field expression, which the path walk never sees.
#[test]
fn test_surface_arch_6_public_surface_census_counts_a_method_call_receiver() -> Result<()> {
    // -- Setup & Fixtures
    let root = support::fixture_root("arch-6-method-receiver")?;
    support::write_source(&root, PUBLISHER, SOURCE, LONELY)?;
    support::write_source(&root, CONSUMER, SOURCE, METHOD_CALL_SOURCE)?;
    let ws = Workspace::from_root(&root)?;

    // -- Exec
    let found = arch_6_public_surface_consumer_census(&ws);

    // -- Check
    assert!(
        found.is_empty(),
        "a receiver call consumes the name: {found:#?}"
    );
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

// region:    --- Impl-member drivers

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

/// WHEN the walk reports surface in a crate the record does not name, THEN the
/// partition guard SHALL fail: no crate may be left unrecorded.
#[test]
fn test_surface_arch_6_partition_flags_an_unrecorded_crate() -> Result<()> {
    // -- Setup & Fixtures
    let unrecorded = Violation {
        assertion: ARCH_6,
        site: "agnt-unknown/src/mod.rs:1".to_string(),
        detail: "`fresh` has no consumer outside `agnt-unknown`".to_string(),
    };

    // -- Exec
    let result = partition(&[unrecorded]);

    // -- Check
    assert!(
        result.is_err(),
        "a crate the record does not name must fail the partition guard"
    );
    Ok(())
}

// endregion: --- Impl-member drivers

// region:    --- Support

/// The violations whose site sits under `krate`'s directory.
fn crate_rows(found: &[Violation], krate: &str) -> Vec<Violation> {
    let prefix = format!("{krate}/");
    found
        .iter()
        .filter(|violation| violation.site.starts_with(&prefix))
        .cloned()
        .collect()
}

/// WHEN the walk reports a crate the record does not name, or the record names a
/// crate twice, THEN the partition SHALL fail.
fn partition(found: &[Violation]) -> Result<()> {
    for violation in found {
        let krate = violation.site.split('/').next().unwrap_or_default();
        if !ARCH_6_CRATES.iter().any(|(name, _)| *name == krate) {
            return Err(format!(
                "`{krate}` is unrecorded: the walk reports surface in a crate with no baseline file"
            )
            .into());
        }
    }
    let mut names: Vec<&str> = ARCH_6_CRATES.iter().map(|(name, _)| *name).collect();
    let recorded = names.len();
    names.sort_unstable();
    names.dedup();
    if names.len() != recorded {
        return Err("a crate is recorded twice".into());
    }
    Ok(())
}

// endregion: --- Support
