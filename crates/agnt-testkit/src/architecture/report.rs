//! The violation record and the ratchet that compares it to a recorded baseline.

use std::collections::BTreeSet;
use std::fmt;

// region:    --- Types

/// One broken assertion: its id, the site that breaks it, and what is wrong
/// there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// The assertion the site breaks, e.g. `ARCH-12`.
    pub assertion: &'static str,
    /// Where it breaks, e.g. `agnt-kernel/Cargo.toml:7`.
    pub site: String,
    /// What breaks there.
    pub detail: String,
}

/// The difference between what an assertion found and what its baseline
/// records, in both directions.
///
/// `added` holds the violations no row records; `stale` holds the rows no
/// violation renders. The diff is data: a driver that must report several
/// comparisons at once collects these and renders them together instead of
/// aborting on the first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaselineDiff {
    /// The violations the baseline does not record.
    pub added: Vec<String>,
    /// The rows no violation renders.
    pub stale: Vec<String>,
}

impl BaselineDiff {
    /// Whether both directions are empty — the violation set equals the record.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.stale.is_empty()
    }
}

// endregion: --- Types

// region:    --- Display

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            assertion,
            site,
            detail,
        } = self;
        write!(f, "{assertion} {site}: {detail}")
    }
}

impl fmt::Display for BaselineDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  new violations:\n{}\n  stale baseline entries:\n{}",
            listed(&self.added),
            listed(&self.stale)
        )
    }
}

// endregion: --- Display

// region:    --- Functions

/// WHEN an assertion's violation set differs from its baseline in either
/// direction, THEN the comparison SHALL fail.
///
/// A violation is identified by its rendered `Display` form, which carries the
/// assertion, the site and the detail. Set equality is exact: a violation that
/// is not recorded fails, and a recorded violation that no longer occurs fails
/// until its entry is removed. The comparison asserts here, on its own diff: a
/// driver that must report several comparisons at once calls
/// [`baseline_diff`] per comparison and asserts once.
pub fn assert_matches_baseline(assertion: &'static str, found: &[Violation], baseline: &[&str]) {
    let diff = baseline_diff(found, baseline);

    assert!(diff.is_empty(), "{assertion} baseline mismatch\n{diff}");
}

/// The two directional differences between `found` and `baseline`, collected
/// rather than asserted.
///
/// A violation is identified by its rendered `Display` form. Set equality is
/// exact, as [`assert_matches_baseline`] states it; this call only answers with
/// what differs.
pub fn baseline_diff(found: &[Violation], baseline: &[&str]) -> BaselineDiff {
    let found: BTreeSet<String> = found.iter().map(ToString::to_string).collect();
    let baseline: BTreeSet<String> = baseline.iter().map(|entry| entry.to_string()).collect();
    BaselineDiff {
        added: found.difference(&baseline).cloned().collect(),
        stale: baseline.difference(&found).cloned().collect(),
    }
}

// endregion: --- Functions

// region:    --- Support

/// Render `entries` as an indented, one-per-line list.
fn listed(entries: &[String]) -> String {
    if entries.is_empty() {
        return "    (none)".to_string();
    }
    entries
        .iter()
        .map(|entry| format!("    {entry}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// endregion: --- Support
