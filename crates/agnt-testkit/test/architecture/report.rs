//! The ratchet: exact set equality, both directions.

use crate::architecture::{BaselineDiff, Violation, assert_matches_baseline, baseline_diff};

use super::support::Result;

// region:    --- Fixtures

/// The assertion these drivers exercise.
const ASSERTION: &str = "ARCH-12";

/// The rendered form of [`RECORDED`].
const RECORDED_TEXT: &str = "ARCH-12 agnt-kernel/Cargo.toml:5: `dependencies` lists `agnt-kernel`";

// endregion: --- Fixtures

// region:    --- Support

/// The violation [`RECORDED_TEXT`] renders.
fn recorded() -> Violation {
    Violation {
        assertion: ASSERTION,
        site: "agnt-kernel/Cargo.toml:5".to_string(),
        detail: "`dependencies` lists `agnt-kernel`".to_string(),
    }
}

// endregion: --- Support

// region:    --- Drivers

/// WHEN the violation set equals the baseline, THE comparison SHALL pass, and
/// THE violation SHALL render as its assertion, site and detail.
#[test]
fn test_report_assert_matches_baseline_accepts_an_exact_match() -> Result<()> {
    // -- Setup & Fixtures
    let found = [recorded()];
    let baseline = [RECORDED_TEXT];

    // -- Exec
    assert_matches_baseline(ASSERTION, &found, &baseline);

    // -- Check
    assert_eq!(found[0].to_string(), RECORDED_TEXT);
    Ok(())
}

/// WHEN a violation is not in the baseline, THEN the comparison SHALL fail.
#[test]
#[should_panic(expected = "new violations")]
fn test_report_assert_matches_baseline_rejects_an_unrecorded_violation() {
    // -- Setup & Fixtures
    let found = [recorded()];
    let baseline: [&str; 0] = [];

    // -- Exec & Check
    assert_matches_baseline(ASSERTION, &found, &baseline);
}

/// WHEN the baseline records a violation that no longer occurs, THEN the
/// comparison SHALL fail until the entry is removed.
#[test]
#[should_panic(expected = "stale baseline entries")]
fn test_report_assert_matches_baseline_rejects_a_stale_baseline_entry() {
    // -- Setup & Fixtures
    let found: [Violation; 0] = [];
    let baseline = [RECORDED_TEXT];

    // -- Exec & Check
    assert_matches_baseline(ASSERTION, &found, &baseline);
}

// endregion: --- Drivers

// region:    --- Baseline-diff drivers

/// WHEN a violation is unrecorded and a recorded row no longer occurs, THEN THE
/// `baseline_diff` function SHALL carry both directions.
#[test]
fn test_report_baseline_diff_carries_both_directions() -> Result<()> {
    // -- Setup & Fixtures
    let found = [recorded()];
    let unrecorded = "ARCH-12 agnt-kernel/Cargo.toml:9: `dev-dependencies` lists `agnt-kernel`";

    // -- Exec
    let diff: BaselineDiff = baseline_diff(&found, &[unrecorded]);

    // -- Check
    assert!(!diff.is_empty(), "the two directions must not cancel out");
    assert_eq!(diff.added, [RECORDED_TEXT.to_string()]);
    assert_eq!(diff.stale, [unrecorded.to_string()]);
    Ok(())
}

/// WHEN the violation set equals the baseline, THEN THE `baseline_diff` function
/// SHALL answer with an empty diff.
#[test]
fn test_report_baseline_diff_is_empty_on_an_exact_match() -> Result<()> {
    // -- Setup & Fixtures
    let found = [recorded()];

    // -- Exec
    let diff = baseline_diff(&found, &[RECORDED_TEXT]);

    // -- Check
    assert!(diff.is_empty(), "an exact match carries no mismatch");
    assert!(diff.added.is_empty());
    assert!(diff.stale.is_empty());
    Ok(())
}

// endregion: --- Baseline-diff drivers
