//! The pure half: the direction rules, pinned with injected inputs.
//!
//! No git, no file system, no process. Each driver states one rule of
//! [`crate::architecture::ledger_growth`], and [`super::driver`] applies them to
//! the real ledgers.

use crate::architecture::{Ledger, LedgerProbe, ledger_growth, ledger_key};

use super::super::support::Result;

// region:    --- Fixtures

/// The ledger file the pure fixtures record.
const LEDGER: &str = "crates/agnt-testkit/test/architecture/baselines_arch6/agnt_runtime.rs";

/// A recorded row.
const ROW: &str = "ARCH-6 agnt-runtime/src/a.rs:12: `Alpha` has no consumer outside `agnt-runtime`";

/// The same row after nine lines were inserted above its site.
const DRIFTED: &str =
    "ARCH-6 agnt-runtime/src/a.rs:21: `Alpha` has no consumer outside `agnt-runtime`";

/// A second row, in the same source file as [`ROW`].
const SIBLING: &str =
    "ARCH-6 agnt-runtime/src/a.rs:40: `Beta` has no consumer outside `agnt-runtime`";

/// A row in another source file.
const ELSEWHERE: &str =
    "ARCH-6 agnt-runtime/src/b.rs:7: `Gamma` has no consumer outside `agnt-runtime`";

/// [`ROW`] in another source file: the same item and the same claim, a new path.
const RELOCATED: &str =
    "ARCH-6 agnt-runtime/src/b.rs:21: `Alpha` has no consumer outside `agnt-runtime`";

// endregion: --- Fixtures

// region:    --- Drivers

/// WHEN a row carries a line number, THEN its key SHALL drop the line and keep
/// the rest, so line drift is not a different row.
#[test]
fn test_monotonicity_ledger_key_strips_the_line_number() -> Result<()> {
    // -- Setup & Fixtures
    let key = ledger_key(ROW);

    // -- Exec & Check
    assert_eq!(
        key,
        "ARCH-6 agnt-runtime/src/a.rs: `Alpha` has no consumer outside `agnt-runtime`"
    );
    assert_eq!(ledger_key(DRIFTED), key, "line drift is the same row");
    assert_eq!(
        ledger_key("a row with no line segment"),
        "a row with no line segment",
        "a row without a `:<digits>:` segment is its own key"
    );
    Ok(())
}

/// WHEN the tree holds a key the branch point did not, THEN the check SHALL
/// fail and name the added key.
#[test]
fn test_monotonicity_ledger_growth_flags_an_added_row() -> Result<()> {
    // -- Setup & Fixtures
    let base = [ledger(&[ROW])];
    let tree = [ledger(&[ROW, ELSEWHERE])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, false);

    // -- Check
    assert_eq!(grown.len(), 1, "{grown:#?}");
    assert_eq!(grown[0].file(), LEDGER);
    assert_eq!(grown[0].rows().len(), 1);
    assert!(
        grown[0].rows()[0].contains("Gamma"),
        "the report must name the added key: {grown:#?}"
    );
    Ok(())
}

/// WHEN a row leaves the ledger, THEN the check SHALL pass: a shrink is the
/// direction the ratchet allows.
#[test]
fn test_monotonicity_ledger_growth_passes_on_a_shrink() -> Result<()> {
    // -- Setup & Fixtures
    let base = [ledger(&[ROW, SIBLING])];
    let tree = [ledger(&[ROW])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, false);

    // -- Check
    assert!(grown.is_empty(), "a drain is not growth: {grown:#?}");
    Ok(())
}

/// WHEN every row keeps its key and only its line moved, THEN the check SHALL
/// pass: line drift is not growth.
#[test]
fn test_monotonicity_ledger_growth_passes_on_line_drift() -> Result<()> {
    // -- Setup & Fixtures
    let base = [ledger(&[ROW, SIBLING])];
    let tree = [ledger(&[DRIFTED, SIBLING])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, false);

    // -- Check
    assert!(grown.is_empty(), "line drift is not growth: {grown:#?}");
    Ok(())
}

/// WHEN the branch changes the walk, THEN growth SHALL pass: a re-measure
/// re-records rows, and this is the branch allowed to.
#[test]
fn test_monotonicity_ledger_growth_passes_when_the_walk_changed() -> Result<()> {
    // -- Setup & Fixtures
    let base = [ledger(&[ROW])];
    let tree = [ledger(&[ROW, ELSEWHERE])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, true);

    // -- Check
    assert!(grown.is_empty(), "a re-measure may grow: {grown:#?}");
    Ok(())
}

/// WHEN the ledger file does not exist at the branch point, THEN its rows SHALL
/// pass: a new crate's first census has no base to grow from.
#[test]
fn test_monotonicity_ledger_growth_passes_on_a_new_ledger_file() -> Result<()> {
    // -- Setup & Fixtures
    let base: Vec<Ledger> = Vec::new();
    let tree = [ledger(&[ROW, ELSEWHERE])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, false);

    // -- Check
    assert!(grown.is_empty(), "a first census is not growth: {grown:#?}");
    Ok(())
}

/// WHEN a key is replaced by another key in the same source file, THEN the
/// check SHALL pass: a rename in place is not growth.
#[test]
fn test_monotonicity_ledger_growth_passes_on_a_same_file_swap() -> Result<()> {
    // -- Setup & Fixtures
    let base = [ledger(&[ROW])];
    let tree = [ledger(&[SIBLING])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, false);

    // -- Check
    assert!(grown.is_empty(), "a same-file swap is a rename: {grown:#?}");
    Ok(())
}

/// WHEN a row keeps its item and its claim and changes source file, THEN the
/// check SHALL pass: a file split moved the site, not the item.
#[test]
fn test_monotonicity_ledger_growth_passes_on_a_cross_file_move() -> Result<()> {
    // -- Setup & Fixtures
    let base = [ledger(&[ROW, SIBLING])];
    let tree = [ledger(&[RELOCATED, SIBLING])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, false);

    // -- Check
    assert!(
        grown.is_empty(),
        "a cross-file move is not growth: {grown:#?}"
    );
    Ok(())
}

/// WHEN a row of another detail arrives in a source file that took a moved row,
/// THEN the check SHALL fail: the move exempts the row it carries, not the file
/// it lands in.
#[test]
fn test_monotonicity_ledger_growth_flags_a_fresh_row_beside_a_moved_row() -> Result<()> {
    // -- Setup & Fixtures
    let base = [ledger(&[ROW])];
    let tree = [ledger(&[RELOCATED, ELSEWHERE])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, false);

    // -- Check
    assert_eq!(grown.len(), 1, "{grown:#?}");
    assert_eq!(grown[0].rows().len(), 1, "{grown:#?}");
    assert!(
        grown[0].rows()[0].contains("Gamma"),
        "the report must name the fresh row: {grown:#?}"
    );
    Ok(())
}

/// WHEN a row keeps its detail but the ledger gave nothing up, THEN the check
/// SHALL fail: a move is a give-up and an arrival, so a second site of a kept
/// row is growth.
#[test]
fn test_monotonicity_ledger_growth_flags_a_second_site_of_a_kept_row() -> Result<()> {
    // -- Setup & Fixtures
    let base = [ledger(&[ROW])];
    let tree = [ledger(&[ROW, RELOCATED])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, false);

    // -- Check
    assert_eq!(grown.len(), 1, "{grown:#?}");
    assert_eq!(grown[0].rows().len(), 1, "{grown:#?}");
    Ok(())
}

/// WHEN a key is replaced by a key of another detail in ANOTHER source file,
/// THEN the check SHALL fail: a different item in a new file is not a move.
#[test]
fn test_monotonicity_ledger_growth_flags_a_row_of_another_detail_in_another_file() -> Result<()> {
    // -- Setup & Fixtures
    let base = [ledger(&[ROW])];
    let tree = [ledger(&[ELSEWHERE])];

    // -- Exec
    let grown = ledger_growth(&base, &tree, false);

    // -- Check
    assert_eq!(grown.len(), 1, "{grown:#?}");
    assert_eq!(grown[0].rows().len(), 1);
    Ok(())
}

/// WHEN git is absent, the checkout is shallow, or `main` is absent, THEN the
/// driver SHALL skip with a printed reason and SHALL NOT fail.
#[test]
fn test_monotonicity_ledger_skip_reason_covers_an_unusable_checkout() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (
            LedgerProbe {
                git: false,
                shallow: false,
                main: true,
            },
            true,
        ),
        (
            LedgerProbe {
                git: true,
                shallow: true,
                main: true,
            },
            true,
        ),
        (
            LedgerProbe {
                git: true,
                shallow: false,
                main: false,
            },
            true,
        ),
        (
            LedgerProbe {
                git: true,
                shallow: false,
                main: true,
            },
            false,
        ),
    ];

    // -- Exec & Check
    for (probe, skipped) in cases {
        let reason = probe.skip_reason();
        assert_eq!(reason.is_some(), skipped, "{probe:?}: {reason:?}");
        if let Some(reason) = reason {
            assert!(!reason.is_empty(), "{probe:?} must state why it skips");
        }
    }
    Ok(())
}

// endregion: --- Drivers

// region:    --- Support

/// A ledger of [`LEDGER`] holding `rows`.
fn ledger(rows: &[&str]) -> Ledger {
    Ledger::new(LEDGER, rows.iter().copied())
}

// endregion: --- Support
