//! The ledger direction: a recorded baseline only ever shrinks.
//!
//! The ratchet's other half. [`assert_matches_baseline`] compares a measured set
//! to its record in both directions, so a fresh violation fails — but a record
//! that grows passes, because the measurement grew with it. A developer who adds
//! surface and records its row in the same commit walks through that hole. This
//! check closes it: the rows the tree records are compared to the rows the
//! branch point recorded, and a row the branch point did not hold is growth.
//!
//! Both ledger shapes — the per-crate `ROWS`/`MEMBER_ROWS` files and the
//! assertion-indexed `BASELINES` table — take one rule. A row is
//! `… <source>:<line>: …`, and the line drifts legitimately when code above the
//! site moves, so [`ledger_key`] strips the first `:<digits>:` segment and the
//! remainder is the key. The census index (`baselines_arch6/mod.rs`) is the
//! crate list, not a ledger: it records no rows of its own, so it is never read
//! as one.
//!
//! Four exemptions, and no fifth; the driver states all four in its failure:
//!
//! - The ledger file is absent at the branch point. A new crate's first census
//!   has no base to grow from, and a renamed file's rows reappear under a path
//!   the branch point never held — the old path's disappearance is the shrink
//!   half.
//! - The branch changes the walk. A re-measure re-records rows, and the branch
//!   that changes the walk's own sources is the branch allowed to. The exemption
//!   is all-or-nothing: one touched file under the walk exempts every ledger,
//!   not only the crates the change re-measured.
//! - The same source file gave up a key in the same comparison. A type renamed
//!   in place, or a row moved within its file, is a rename, not growth.
//! - The same row changed source file in the same comparison. A file split moves
//!   a recorded site into a submodule without moving the item, so the row keeps
//!   its item and its claim and only the path before the colon changes. A key
//!   whose detail a given-up key of the same ledger already carried is that row
//!   relocated, not growth.
//!
//! Accepted limits, bound for bound:
//!
//! - A drain and a growth that land in one source file can pass as a swap. This
//!   check catches net growth; the review side's normalized baseline diff
//!   catches a swap that nets to zero.
//! - Two rows in one file that differ only in line number collapse to one key,
//!   so a second declaration of an already-recorded name in the same file is
//!   not growth.
//! - The comparison is per ledger file, so a row that moves between two ledger
//!   files is growth in the destination file.
//! - A drain and a growth that carry the same detail in one ledger can pass as a
//!   cross-file move, as one in the same file can pass as a rename. Only the
//!   detail is paired, not the count: two rows that keep the same item and claim
//!   are one row's shape, and the walk reports each site it finds.
//!
//! [`assert_matches_baseline`]: crate::architecture::assert_matches_baseline
//!
//! The check takes injected inputs and runs no process. The git reader lives in
//! `test/architecture/monotonicity/` — the suite's one sanctioned git reader,
//! stated in the module contract of [`crate::architecture`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// region:    --- Types

/// One ledger file and the rows it records, as the file writes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    file: String,
    rows: Vec<String>,
}

/// One ledger file the working tree records more keys in than the branch point
/// did: the file, and the keys it added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Growth {
    file: String,
    rows: Vec<String>,
}

/// The three facts a driver probes from git before it compares a ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerProbe {
    /// Whether git runs in this checkout at all.
    pub git: bool,
    /// Whether the checkout is shallow.
    pub shallow: bool,
    /// Whether a `main` ref exists to compare against.
    pub main: bool,
}

// endregion: --- Types

// region:    --- Display

impl fmt::Display for Growth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} grew by {} row(s)", self.file, self.rows.len())?;
        for row in &self.rows {
            write!(f, "\n    {row}")?;
        }
        Ok(())
    }
}

// endregion: --- Display

// region:    --- Functions

/// WHEN a ledger key is in the working tree and not at the branch point, THEN
/// the ledger SHALL be reported as grown, unless the walk changed, the ledger
/// file is absent at the branch point, the same source file gave up a key, or a
/// key of the same detail left another source file.
pub fn ledger_growth(base: &[Ledger], tree: &[Ledger], walk_changed: bool) -> Vec<Growth> {
    if walk_changed {
        return Vec::new();
    }
    let base: BTreeMap<&str, BTreeSet<String>> = base
        .iter()
        .map(|ledger| (ledger.file(), keys(&ledger.rows)))
        .collect();
    let mut growth = Vec::new();
    for ledger in tree {
        let Some(recorded) = base.get(ledger.file()) else {
            continue;
        };
        let current = keys(&ledger.rows);
        let removed = recorded.difference(&current);
        let given_up = files(removed.clone());
        let moved = details(removed);
        let rows: Vec<String> = current
            .difference(recorded)
            .filter(|key| !given_up.contains(file_of(key)))
            .filter(|key| !moved.contains(detail_of(key)))
            .cloned()
            .collect();
        if !rows.is_empty() {
            growth.push(Growth {
                file: ledger.file().to_string(),
                rows,
            });
        }
    }
    growth
}

/// WHEN a recorded row carries a line number, THEN its key SHALL be the row
/// with the first `:<digits>:` segment removed.
///
/// The line is not part of what a row claims: code inserted above the site
/// moves the line without moving the item, the member or the defect.
pub fn ledger_key(row: &str) -> String {
    let Some((file, rest)) = row.split_once(':') else {
        return row.to_string();
    };
    let Some((line, tail)) = rest.split_once(':') else {
        return row.to_string();
    };
    if line.is_empty() || !line.chars().all(|character| character.is_ascii_digit()) {
        return row.to_string();
    }
    format!("{file}:{tail}")
}

// endregion: --- Functions

// region:    --- Ledger: Constructors

impl Ledger {
    /// The ledger file at `file` and the rows it records.
    pub fn new(file: impl Into<String>, rows: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            file: file.into(),
            rows: rows.into_iter().map(Into::into).collect(),
        }
    }
}

// endregion: --- Ledger: Constructors

// region:    --- Ledger: Accessors

impl Ledger {
    /// The repository path of the file that records the rows.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Every row the file records.
    pub fn rows(&self) -> &[String] {
        &self.rows
    }
}

// endregion: --- Ledger: Accessors

// region:    --- Growth: Accessors

impl Growth {
    /// The repository path of the ledger file that grew.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The keys the branch point did not record.
    pub fn rows(&self) -> &[String] {
        &self.rows
    }
}

// endregion: --- Growth: Accessors

// region:    --- LedgerProbe: Accessors

impl LedgerProbe {
    /// WHY the driver cannot compare a ledger in this checkout, or `None` when
    /// it can.
    pub fn skip_reason(&self) -> Option<&'static str> {
        if !self.git {
            return Some(NO_GIT);
        }
        if self.shallow {
            return Some(SHALLOW_CHECKOUT);
        }
        if !self.main {
            return Some(NO_MAIN);
        }
        None
    }
}

// endregion: --- LedgerProbe: Accessors

// region:    --- Support

/// Why git cannot be read.
const NO_GIT: &str = "git is unavailable, so the branch point cannot be read";

/// Why the branch point is not reachable.
const SHALLOW_CHECKOUT: &str = "the checkout is shallow, so the branch point is not in the history";

/// Why there is nothing to compare against.
const NO_MAIN: &str = "no `main` ref exists, so there is no branch point to compare against";

/// The keys `rows` compares by.
fn keys(rows: &[String]) -> BTreeSet<String> {
    rows.iter().map(|row| ledger_key(row)).collect()
}

/// The source files `keys` name.
fn files<'a>(keys: impl Iterator<Item = &'a String>) -> BTreeSet<&'a str> {
    keys.map(|key| file_of(key)).collect()
}

/// The details `keys` name.
fn details<'a>(keys: impl Iterator<Item = &'a String>) -> BTreeSet<&'a str> {
    keys.map(|key| detail_of(key)).collect()
}

/// The source file a key names: what precedes its first colon.
fn file_of(key: &str) -> &str {
    key.split_once(':').map_or(key, |(file, _)| file)
}

/// The detail a key names: what follows its first colon. A row that keeps its
/// item and its claim but changes source file names the same detail.
fn detail_of(key: &str) -> &str {
    key.split_once(':').map_or(key, |(_, detail)| detail)
}

// endregion: --- Support
