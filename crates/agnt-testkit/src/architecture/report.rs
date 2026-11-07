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

// endregion: --- Display

// region:    --- Functions

/// WHEN an assertion's violation set differs from its baseline in either
/// direction, THEN the comparison SHALL fail.
///
/// A violation is identified by its rendered `Display` form, which carries the
/// assertion, the site and the detail. Set equality is exact: a violation that
/// is not recorded fails, and a recorded violation that no longer occurs fails
/// until its entry is removed.
pub fn assert_matches_baseline(assertion: &'static str, found: &[Violation], baseline: &[&str]) {
    let found: BTreeSet<String> = found.iter().map(ToString::to_string).collect();
    let baseline: BTreeSet<String> = baseline.iter().map(|entry| entry.to_string()).collect();
    let added: Vec<&String> = found.difference(&baseline).collect();
    let stale: Vec<&String> = baseline.difference(&found).collect();

    assert!(
        added.is_empty() && stale.is_empty(),
        "{assertion} baseline mismatch\n  new violations:\n{}\n  stale baseline entries:\n{}",
        listed(&added),
        listed(&stale),
    );
}

// endregion: --- Functions

// region:    --- Support

/// Render `entries` as an indented, one-per-line list.
fn listed(entries: &[&String]) -> String {
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
