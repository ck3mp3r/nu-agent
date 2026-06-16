use super::core::{PatchOp, PatchRange, PatchSummary, apply_line_range_patch_batch, version_token};
use std::fs;
use tempfile::tempdir;

fn assert_conflict_and_unchanged(
    summary: PatchSummary,
    expected_version: &str,
    current: &str,
    file: &std::path::Path,
) {
    assert_eq!(summary.operation_count, 0);
    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert!(!summary.noop);
    assert!(summary.conflict);
    assert_eq!(summary.previous_version, current);
    assert_eq!(summary.new_version, current);
    assert_eq!(summary.expected_version, expected_version);
    assert_eq!(
        fs::read_to_string(file).expect("read"),
        "alpha\nbeta\ngamma\ndelta\n"
    );
}

#[test]
fn apply_line_range_patch_batch_requires_expected_version() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("missing-version.txt");
    fs::write(&file, "alpha\nbeta\n").expect("seed file");

    let err = apply_line_range_patch_batch(
        &file,
        None,
        vec![PatchOp {
            range: PatchRange::single(1),
            replacement: "changed\n".to_string(),
        }],
    )
    .expect_err("expected validation error");

    assert_eq!(
        err.to_string(),
        "missing expected_version for mutating operation"
    );
    assert_eq!(fs::read_to_string(&file).expect("read"), "alpha\nbeta\n");
}

#[test]
fn apply_line_range_patch_batch_conflict_on_version_mismatch_and_does_not_write() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("conflict.txt");
    let content = "alpha\nbeta\ngamma\ndelta\n";
    fs::write(&file, content).expect("seed file");

    let stale = "stale-version";
    let summary = apply_line_range_patch_batch(
        &file,
        Some(stale),
        vec![PatchOp {
            range: PatchRange::single(2),
            replacement: "BETA\n".to_string(),
        }],
    )
    .expect("conflict summary");

    let current = version_token(content);
    assert_conflict_and_unchanged(summary, stale, &current, &file);
}

#[test]
fn apply_line_range_patch_batch_applies_multiple_ops_in_reverse_order() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("reverse-order.txt");
    let content = "line1\nline2\nline3\nline4\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    let summary = apply_line_range_patch_batch(
        &file,
        Some(&expected_version),
        vec![
            PatchOp {
                range: PatchRange::new(2, 3),
                replacement: "X\nY\n".to_string(),
            },
            PatchOp {
                range: PatchRange::single(4),
                replacement: "tail\n".to_string(),
            },
        ],
    )
    .expect("apply");

    assert_eq!(summary.operation_count, 2);
    assert!(summary.wrote);
    assert!(summary.changed);
    assert!(!summary.conflict);
    assert!(!summary.noop);
    assert_eq!(summary.previous_version, expected_version);
    assert_ne!(summary.new_version, summary.previous_version);
    assert_eq!(
        fs::read_to_string(&file).expect("read"),
        "line1\nX\nY\ntail\n"
    );
}

#[test]
fn apply_line_range_patch_batch_rejects_out_of_bounds_range() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("bounds.txt");
    let content = "line1\nline2\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    let err = apply_line_range_patch_batch(
        &file,
        Some(&expected_version),
        vec![PatchOp {
            range: PatchRange::new(2, 3),
            replacement: "replace\n".to_string(),
        }],
    )
    .expect_err("expected validation error");

    assert!(
        err.to_string()
            .contains("patch range out of bounds: start=2 end=3 total_lines=2")
    );
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
}

#[test]
fn apply_line_range_patch_batch_rejects_overlapping_ranges() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("overlap.txt");
    let content = "a\nb\nc\nd\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    let err = apply_line_range_patch_batch(
        &file,
        Some(&expected_version),
        vec![
            PatchOp {
                range: PatchRange::new(2, 3),
                replacement: "bc\n".to_string(),
            },
            PatchOp {
                range: PatchRange::new(3, 4),
                replacement: "cd\n".to_string(),
            },
        ],
    )
    .expect_err("expected overlap error");

    assert!(
        err.to_string()
            .contains("patch ranges overlap: [2,3] with [3,4]")
    );
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
}

#[test]
fn apply_line_range_patch_batch_noop_returns_deterministic_summary_without_write() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("noop.txt");
    let content = "one\ntwo\nthree\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    let summary = apply_line_range_patch_batch(
        &file,
        Some(&expected_version),
        vec![PatchOp {
            range: PatchRange::single(2),
            replacement: "two\n".to_string(),
        }],
    )
    .expect("apply");

    assert_eq!(summary.operation_count, 1);
    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert!(summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, expected_version);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
}
