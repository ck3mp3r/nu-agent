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
//!
//! The half-specific drivers live one per sub-file: [`declared`] pins what can
//! consume a free item, [`members`] what can consume a `pub` member. Each
//! sub-file pulls the shared imports in with `use super::*;`.

// region:    --- Modules

mod declared;
mod members;

use crate::architecture::{
    Violation, Workspace, arch_6_impl_member_census, arch_6_public_surface_consumer_census,
};

use super::support::{ARCH_6, ARCH_6_CRATES, ARCH_6_MEMBER_CRATES, Result};
use super::surface_baseline_diff::{arch_6_baseline_mismatches, render_arch_6_mismatches};

// endregion: --- Modules

// region:    --- Fixtures

/// The crate that publishes.
const PUBLISHER: &str = "agnt-runtime";

/// The crate that consumes.
const CONSUMER: &str = "agnt-tui";

/// The source both fixture crates write.
const SOURCE: &str = "mod.rs";

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
    // Every half is compared before the driver fails: one drift used to abort the
    // walk where it stood, so each round trip unmasked exactly one more crate.
    let mismatches = arch_6_baseline_mismatches(&items, &members);
    if let Some(report) = render_arch_6_mismatches(&mismatches) {
        panic!("{report}");
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

// endregion: --- Drivers

// region:    --- Support

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
