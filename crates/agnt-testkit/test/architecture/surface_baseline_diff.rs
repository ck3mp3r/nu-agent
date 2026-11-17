//! The ARCH-6 baseline comparison of every crate half, and the report it draws.
//!
//! The driver walks ten crates and two halves each. A mismatched half used to
//! panic where it stood, so every later half stayed unevaluated and each round
//! trip unmasked exactly one more drift. The comparison is collected here
//! instead: every half is measured, and one report names them all.

use crate::architecture::{BaselineDiff, Violation, baseline_diff};

use super::support::{ARCH_6, ARCH_6_CRATES, ARCH_6_MEMBER_CRATES, Result};

// region:    --- Types

/// One crate half that differs from the rows recorded for it.
#[derive(Debug)]
pub(super) struct HalfMismatch {
    /// The crate the half belongs to, as the record names it.
    pub(super) krate: &'static str,
    /// Which half: the declared items or the impl members.
    pub(super) half: &'static str,
    /// What differs in that half.
    pub(super) diff: BaselineDiff,
}

// endregion: --- Types

// region:    --- Functions

/// Compare every crate's declared-items half and impl-members half, and answer
/// with the halves that differ, in crate order.
///
/// No half stops the walk: a mismatch is collected and the next half is
/// measured, so one report names every drift instead of the first.
pub(super) fn arch_6_baseline_mismatches(
    items: &[Violation],
    members: &[Violation],
) -> Vec<HalfMismatch> {
    let mut mismatches = Vec::new();
    for ((krate, item_rows), (member_crate, member_rows)) in
        ARCH_6_CRATES.iter().zip(ARCH_6_MEMBER_CRATES.iter())
    {
        assert_eq!(krate, member_crate, "the two halves must be in crate order");
        let item_diff = baseline_diff(&crate_rows(items, krate), item_rows);
        if !item_diff.is_empty() {
            mismatches.push(HalfMismatch {
                krate,
                half: DECLARED_ITEMS,
                diff: item_diff,
            });
        }
        let member_diff = baseline_diff(&crate_rows(members, krate), member_rows);
        if !member_diff.is_empty() {
            mismatches.push(HalfMismatch {
                krate,
                half: IMPL_MEMBERS,
                diff: member_diff,
            });
        }
    }
    mismatches
}

/// Draw one report naming every mismatched half, or `None` when every half
/// matches its recorded rows.
pub(super) fn render_arch_6_mismatches(mismatches: &[HalfMismatch]) -> Option<String> {
    if mismatches.is_empty() {
        return None;
    }
    let blocks: Vec<String> = mismatches
        .iter()
        .map(|HalfMismatch { krate, half, diff }| {
            format!(
                "  {krate} ({half}):\n    {}",
                diff.to_string().replace('\n', "\n    ")
            )
        })
        .collect();
    Some(format!(
        "{ARCH_6} baseline mismatch in {} crate halves:\n{}",
        mismatches.len(),
        blocks.join("\n")
    ))
}

// endregion: --- Functions

// region:    --- Support

/// The label the declared-items half carries in the report.
const DECLARED_ITEMS: &str = "declared items";

/// The label the impl-members half carries in the report.
const IMPL_MEMBERS: &str = "impl members";

/// The violations whose site sits under `krate`'s directory.
fn crate_rows(found: &[Violation], krate: &str) -> Vec<Violation> {
    let prefix = format!("{krate}/");
    found
        .iter()
        .filter(|violation| violation.site.starts_with(&prefix))
        .cloned()
        .collect()
}

// endregion: --- Support

// region:    --- Test Support

/// The violation one recorded row renders.
fn violation_at(site: &str) -> Violation {
    Violation {
        assertion: ARCH_6,
        site: site.to_string(),
        detail: "`fresh` has no consumer outside its crate".to_string(),
    }
}

/// The violations the recorded rows name, one per row.
fn recorded(rows: &[&str]) -> Vec<Violation> {
    rows.iter()
        .filter_map(|row| {
            let rest = row.strip_prefix("ARCH-6 ")?;
            let (site, detail) = rest.split_once(": ")?;
            Some(Violation {
                assertion: ARCH_6,
                site: site.to_string(),
                detail: detail.to_string(),
            })
        })
        .collect()
}

// endregion: --- Support

// region:    --- Drivers

/// WHEN several crate halves differ from their baselines, THEN THE comparison
/// SHALL answer with every mismatched half, not only the first.
///
/// Three crates each carry one unrecorded row — two on their declared-items
/// half, one on its impl-members half — so a report naming all three proves no
/// half stopped the walk before a later one.
#[test]
fn test_surface_baseline_diff_names_every_mismatched_crate_half() -> Result<()> {
    // -- Setup & Fixtures
    let mut items: Vec<Violation> = ARCH_6_CRATES
        .iter()
        .flat_map(|(_, rows)| recorded(rows))
        .collect();
    items.push(violation_at("agnt-runtime/src/fresh.rs:1"));
    items.push(violation_at("agnt-kernel/src/fresh.rs:1"));
    let mut members: Vec<Violation> = ARCH_6_MEMBER_CRATES
        .iter()
        .flat_map(|(_, rows)| recorded(rows))
        .collect();
    members.push(violation_at("agnt-tui/src/fresh.rs:1"));

    // -- Exec
    let mismatches = arch_6_baseline_mismatches(&items, &members);
    let report = render_arch_6_mismatches(&mismatches);

    // -- Check
    assert_eq!(
        mismatches.len(),
        3,
        "every mismatched half must be compared: {mismatches:#?}"
    );
    let report = report.ok_or("the mismatched halves must draw a report")?;
    for krate in ["agnt-runtime", "agnt-kernel", "agnt-tui"] {
        assert!(
            report.contains(krate),
            "the report must name {krate}: {report}"
        );
    }
    assert_eq!(
        report.matches("new violations:").count(),
        3,
        "each mismatched half carries its own directions: {report}"
    );
    Ok(())
}

/// WHEN a crate's declared-items half differs, THEN THE comparison SHALL still
/// evaluate that crate's impl-members half.
///
/// Both halves belong to one crate: an unrecorded item and a member row that no
/// longer occurs. The member half is only visible because the item half's
/// mismatch no longer aborts the walk, and no other crate reports.
#[test]
fn test_surface_baseline_diff_keeps_the_member_half_of_a_mismatched_crate() -> Result<()> {
    // -- Setup & Fixtures
    let mut items: Vec<Violation> = ARCH_6_CRATES
        .iter()
        .flat_map(|(_, rows)| recorded(rows))
        .collect();
    items.push(violation_at("agnt-cli/src/fresh.rs:1"));
    let mut members: Vec<Violation> = ARCH_6_MEMBER_CRATES
        .iter()
        .flat_map(|(_, rows)| recorded(rows))
        .collect();
    let recorded_members = members.len();
    members.retain(|member| member.site != "agnt-cli/src/mode/mod.rs:342");
    assert_eq!(
        members.len(),
        recorded_members - 1,
        "the fixture must drop exactly one recorded member row"
    );

    // -- Exec
    let mismatches = arch_6_baseline_mismatches(&items, &members);
    let report = render_arch_6_mismatches(&mismatches);

    // -- Check
    assert_eq!(
        mismatches.len(),
        2,
        "one crate, both halves: {mismatches:#?}"
    );
    let report = report.ok_or("the mismatched halves must draw a report")?;
    assert!(
        report.contains("agnt-cli (declared items)"),
        "the item half must be named: {report}"
    );
    assert!(
        report.contains("agnt-cli (impl members)"),
        "the member half must be named too: {report}"
    );
    Ok(())
}

/// WHEN every half matches, THEN THE comparison SHALL draw no report at all.
#[test]
fn test_surface_baseline_diff_draws_no_report_when_every_half_matches() -> Result<()> {
    // -- Setup & Fixtures
    let mismatches: [HalfMismatch; 0] = [];

    // -- Exec
    let report = render_arch_6_mismatches(&mismatches);

    // -- Check
    assert!(
        report.is_none(),
        "a matching tree must leave the pass path untouched"
    );
    Ok(())
}

// endregion: --- Drivers
