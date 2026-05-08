use super::core::{MutateError, apply_full_content_mutation, version_token};
use std::fs;
use tempfile::tempdir;

fn test_line_count(content: &str) -> usize {
    if content.is_empty() {
        return 0;
    }

    content.split_inclusive('\n').count()
}

#[test]
fn apply_full_content_mutation_matching_version_writes_and_reports_deterministic_summary() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("apply.txt");

    let original = "line1\nline2\n";
    let updated = "line1\nline2\nline3\n";
    fs::write(&file, original).expect("seed file");

    let expected_version = version_token(original);
    let summary = apply_full_content_mutation(&file, Some(&expected_version), updated).expect("apply");

    assert!(summary.wrote);
    assert!(summary.changed);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, version_token(updated));
    assert_eq!(summary.previous_bytes, original.len());
    assert_eq!(summary.new_bytes, updated.len());
    assert_eq!(summary.previous_lines, test_line_count(original));
    assert_eq!(summary.new_lines, test_line_count(updated));
    assert_eq!(fs::read_to_string(&file).expect("read"), updated);
}

#[test]
fn apply_full_content_mutation_stale_version_conflicts_and_does_not_write() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("conflict.txt");

    let original = "before\n";
    let attempted = "after\n";
    fs::write(&file, original).expect("seed file");

    let err = apply_full_content_mutation(&file, Some("stale-version"), attempted)
        .expect_err("expected conflict");
    assert!(matches!(err, MutateError::Conflict(_)));
    assert_eq!(fs::read_to_string(&file).expect("read"), original);
}

#[test]
fn apply_full_content_mutation_missing_expected_version_is_validation_error() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("validation.txt");

    fs::write(&file, "seed\n").expect("seed file");

    let err = apply_full_content_mutation(&file, None, "updated\n")
        .expect_err("expected validation error");
    assert!(matches!(err, MutateError::MissingExpectedVersion));
    assert_eq!(fs::read_to_string(&file).expect("read"), "seed\n");
}

#[test]
fn apply_full_content_mutation_unchanged_content_matching_version_is_deterministic_without_write() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("unchanged.txt");

    let content = "same\ncontent\n";
    fs::write(&file, content).expect("seed file");

    let version = version_token(content);
    let summary = apply_full_content_mutation(&file, Some(&version), content).expect("apply");

    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert_eq!(summary.previous_version, version);
    assert_eq!(summary.new_version, version);
    assert_eq!(summary.previous_bytes, content.len());
    assert_eq!(summary.new_bytes, content.len());
    let lines = test_line_count(content);
    assert_eq!(summary.previous_lines, lines);
    assert_eq!(summary.new_lines, lines);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
}
