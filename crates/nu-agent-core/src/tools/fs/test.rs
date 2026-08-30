use super::core::{
    ConflictError, EditMatchMode, EditOccurrence, EditOperation, FileMetadata, MutateError,
    PatchOp, PatchRange, PatchSummary, ReadRequest, apply_create_file, apply_full_content_mutation,
    apply_line_range_patch_batch, apply_search_replace_edit, atomic_overwrite, check_conflict,
    metadata_for_path, plan_create_file, read_file, version_token,
};
use super::diff::{DiffBounds, compute_edit_unified_diff_bounded};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn test_line_count(content: &str) -> usize {
    if content.is_empty() {
        return 0;
    }

    content.split_inclusive('\n').count()
}

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

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: core
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn version_token_changes_for_different_content_and_stable_for_same_content() {
    let a1 = version_token("hello");
    let a2 = version_token("hello");
    let b = version_token("hello world");

    assert_eq!(a1, a2);
    assert_ne!(a1, b);
}

#[test]
fn check_conflict_detects_mismatch_and_accepts_match() {
    let current = "abc123".to_string();

    assert!(check_conflict(None, &current).is_ok());
    assert!(check_conflict(Some("abc123"), &current).is_ok());

    let err = check_conflict(Some("different"), &current).expect_err("expected conflict");
    assert_eq!(
        err,
        ConflictError {
            expected_version: "different".to_string(),
            current_version: current,
        }
    );
}

#[test]
fn atomic_overwrite_replaces_file_content() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("target.txt");

    fs::write(&file, "before").expect("seed file");
    atomic_overwrite(&file, b"after").map_err(|e| format!("{e:?}"))?;

    let actual = fs::read_to_string(&file).expect("read file");
    assert_eq!(actual, "after");
    Ok(())
}

#[test]
fn metadata_helper_returns_size_and_mtime() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("meta.txt");

    fs::write(&file, "12345").expect("write file");
    let metadata = metadata_for_path(&file).map_err(|e| format!("{e:?}"))?;

    assert_eq!(
        metadata,
        FileMetadata {
            size_bytes: 5,
            modified_unix_seconds: metadata.modified_unix_seconds,
        }
    );
    assert!(metadata.modified_unix_seconds > 0);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: read
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn read_file_without_offset_limit_returns_full_content_and_metadata() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("sample.txt");
    fs::write(&file, "line1\nline2\nline3\n").expect("write file");

    let response = read_file(&file, ReadRequest::default()).map_err(|e| format!("{e:?}"))?;

    assert_eq!(response.content, "line1\nline2\nline3\n");
    assert_eq!(response.total_lines, 3);
    assert_eq!(response.offset, None);
    assert_eq!(response.limit, None);
    assert!(!response.version.is_empty());
    Ok(())
}

#[test]
fn read_file_with_offset_and_limit_returns_window_content_and_metadata() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("window.txt");
    fs::write(&file, "a\nb\nc\nd\n").expect("write file");

    let response = read_file(
        &file,
        ReadRequest {
            offset: Some(1),
            limit: Some(2),
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    assert_eq!(response.content, "b\nc\n");
    assert_eq!(response.total_lines, 4);
    assert_eq!(response.offset, Some(1));
    assert_eq!(response.limit, Some(2));
    assert!(!response.version.is_empty());
    Ok(())
}

#[test]
fn read_file_empty_file_is_deterministic() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").expect("write file");

    let response = read_file(&file, ReadRequest::default()).map_err(|e| format!("{e:?}"))?;

    assert_eq!(response.content, "");
    assert_eq!(response.total_lines, 0);
    assert_eq!(response.offset, None);
    assert_eq!(response.limit, None);
    assert!(!response.version.is_empty());
    Ok(())
}

#[test]
fn read_file_offset_beyond_eof_returns_empty_window_deterministically() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("eof.txt");
    fs::write(&file, "x\ny\n").expect("write file");

    let response = read_file(
        &file,
        ReadRequest {
            offset: Some(10),
            limit: Some(3),
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    assert_eq!(response.content, "");
    assert_eq!(response.total_lines, 2);
    assert_eq!(response.offset, Some(10));
    assert_eq!(response.limit, Some(3));
    assert!(!response.version.is_empty());
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: mutation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn apply_full_content_mutation_matching_version_writes_and_reports_deterministic_summary()
-> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("apply.txt");

    let original = "line1\nline2\n";
    let updated = "line1\nline2\nline3\n";
    fs::write(&file, original).expect("seed file");

    let expected_version = version_token(original);
    let summary = apply_full_content_mutation(&file, Some(&expected_version), updated)
        .map_err(|e| format!("{e:?}"))?;

    assert!(summary.wrote);
    assert!(summary.changed);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, version_token(updated));
    assert_eq!(summary.previous_bytes, original.len());
    assert_eq!(summary.new_bytes, updated.len());
    assert_eq!(summary.previous_lines, test_line_count(original));
    assert_eq!(summary.new_lines, test_line_count(updated));
    assert_eq!(fs::read_to_string(&file).expect("read"), updated);
    Ok(())
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
fn apply_full_content_mutation_unchanged_content_matching_version_is_deterministic_without_write()
-> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("unchanged.txt");

    let content = "same\ncontent\n";
    fs::write(&file, content).expect("seed file");

    let version = version_token(content);
    let summary = apply_full_content_mutation(&file, Some(&version), content)
        .map_err(|e| format!("{e:?}"))?;

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
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: patch
// ═══════════════════════════════════════════════════════════════════════════════

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
fn apply_line_range_patch_batch_conflict_on_version_mismatch_and_does_not_write() -> Result<()> {
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
    .map_err(|e| format!("{e:?}"))?;

    let current = version_token(content);
    assert_conflict_and_unchanged(summary, stale, &current, &file);
    Ok(())
}

#[test]
fn apply_line_range_patch_batch_applies_multiple_ops_in_reverse_order() -> Result<()> {
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
    .map_err(|e| format!("{e:?}"))?;

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
    Ok(())
}

#[test]
fn apply_line_range_patch_batch_handles_ops_in_descending_line_order_without_panic() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("descending-order.txt");
    let content = "line1\nline2\nline3\nline4\nline5\nline6\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    // Operations are sent in descending line order (high line first), which
    // previously caused a panic in apply_patch_operations_in_reverse.
    let summary = apply_line_range_patch_batch(
        &file,
        Some(&expected_version),
        vec![
            PatchOp {
                range: PatchRange::single(5),
                replacement: "FIVE\n".to_string(),
            },
            PatchOp {
                range: PatchRange::new(2, 3),
                replacement: "X\nY\n".to_string(),
            },
        ],
    )
    .map_err(|e| format!("{e:?}"))?;

    assert_eq!(summary.operation_count, 2);
    assert!(summary.wrote);
    assert!(summary.changed);
    assert!(!summary.conflict);
    assert_eq!(
        fs::read_to_string(&file).expect("read"),
        "line1\nX\nY\nline4\nFIVE\nline6\n"
    );
    Ok(())
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
fn apply_line_range_patch_batch_noop_returns_deterministic_summary_without_write() -> Result<()> {
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
    .map_err(|e| format!("{e:?}"))?;

    assert_eq!(summary.operation_count, 1);
    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert!(summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, expected_version);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: edit
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn apply_search_replace_edit_requires_expected_version() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("missing-version.txt");
    fs::write(&file, "alpha beta\n").expect("seed file");

    let err = apply_search_replace_edit(
        &file,
        None,
        &EditOperation {
            search: "alpha".to_string(),
            replacement: "omega".to_string(),
            match_mode: EditMatchMode::Literal,
            occurrence: EditOccurrence::First,
        },
    )
    .expect_err("expected validation error");

    assert!(matches!(err, MutateError::MissingExpectedVersion));
    assert_eq!(fs::read_to_string(&file).expect("read"), "alpha beta\n");
}

#[test]
fn apply_search_replace_edit_conflict_on_stale_version_without_write() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("conflict.txt");
    let content = "alpha beta alpha\n";
    fs::write(&file, content).expect("seed file");

    let summary = apply_search_replace_edit(
        &file,
        Some("stale-version"),
        &EditOperation {
            search: "alpha".to_string(),
            replacement: "omega".to_string(),
            match_mode: EditMatchMode::Literal,
            occurrence: EditOccurrence::All,
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    let current = version_token(content);
    assert_eq!(summary.replacements, 0);
    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert!(!summary.noop);
    assert!(summary.conflict);
    assert_eq!(summary.expected_version, "stale-version");
    assert_eq!(summary.previous_version, current);
    assert_eq!(summary.new_version, current);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
    Ok(())
}

#[test]
fn plan_create_file_returns_plan_for_nonexistent_file() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("new-create.txt");
    let content = "hello world\n";

    let plan = plan_create_file(&file, content).map_err(|e| format!("{e:?}"))?;

    assert!(!plan.conflict);
    assert!(plan.would_change);
    assert!(!plan.noop);
    assert_eq!(plan.replacements, 0);
    assert_eq!(plan.previous_content, "");
    assert_eq!(plan.new_content, content);
    assert_eq!(plan.previous_bytes, 0);
    assert_eq!(plan.new_bytes, content.len());
    assert_eq!(plan.previous_lines, 0);
    assert_eq!(plan.new_lines, 1);
    assert_eq!(plan.previous_version, version_token(""));
    Ok(())
}

#[test]
fn plan_create_file_conflicts_when_file_already_exists() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("exists.txt");
    fs::write(&file, "existing\n").expect("seed");

    let plan = plan_create_file(&file, "new content\n").map_err(|e| format!("{e:?}"))?;

    assert!(plan.conflict);
    assert!(!plan.would_change);
    assert!(!plan.noop);
    assert_eq!(plan.previous_version, version_token("existing\n"));
    Ok(())
}

#[test]
fn apply_create_file_creates_new_file_atomically() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("created-by-apply.txt");
    let content = "brand new file\nline two\n";

    let summary = apply_create_file(&file, content).map_err(|e| format!("{e:?}"))?;

    assert!(summary.wrote);
    assert!(summary.changed);
    assert!(!summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.replacements, 0);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
    assert_eq!(summary.previous_version, version_token(""));
    assert_eq!(summary.new_version, version_token(content));
    Ok(())
}

#[test]
fn apply_create_file_conflicts_when_file_appears_between_plan_and_apply() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("race-condition.txt");
    let content = "intended content\n";

    fs::write(&file, "someone else created it\n").expect("seed");

    let summary = apply_create_file(&file, content).map_err(|e| format!("{e:?}"))?;

    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert!(summary.conflict);
    assert_eq!(
        fs::read_to_string(&file).expect("read"),
        "someone else created it\n"
    );
    Ok(())
}

#[test]
fn apply_search_replace_edit_literal_first_replaces_only_first_match() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("literal-first.txt");
    let content = "alpha beta alpha\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    let summary = apply_search_replace_edit(
        &file,
        Some(&expected_version),
        &EditOperation {
            search: "alpha".to_string(),
            replacement: "omega".to_string(),
            match_mode: EditMatchMode::Literal,
            occurrence: EditOccurrence::First,
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    let expected_content = "omega beta alpha\n";
    assert_eq!(summary.replacements, 1);
    assert!(summary.wrote);
    assert!(summary.changed);
    assert!(!summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.expected_version, expected_version);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, version_token(expected_content));
    assert_eq!(fs::read_to_string(&file).expect("read"), expected_content);
    Ok(())
}

#[test]
fn apply_search_replace_edit_literal_all_replaces_all_matches() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("literal-all.txt");
    let content = "alpha beta alpha\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    let summary = apply_search_replace_edit(
        &file,
        Some(&expected_version),
        &EditOperation {
            search: "alpha".to_string(),
            replacement: "omega".to_string(),
            match_mode: EditMatchMode::Literal,
            occurrence: EditOccurrence::All,
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    let expected_content = "omega beta omega\n";
    assert_eq!(summary.replacements, 2);
    assert!(summary.wrote);
    assert!(summary.changed);
    assert!(!summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.expected_version, expected_version);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, version_token(expected_content));
    assert_eq!(fs::read_to_string(&file).expect("read"), expected_content);
    Ok(())
}

#[test]
fn apply_search_replace_edit_regex_first_replaces_only_first_match() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("regex-first.txt");
    let content = "alpha 1\nalpha 2\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    let summary = apply_search_replace_edit(
        &file,
        Some(&expected_version),
        &EditOperation {
            search: "alpha\\s+\\d".to_string(),
            replacement: "omega".to_string(),
            match_mode: EditMatchMode::Regex,
            occurrence: EditOccurrence::First,
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    let expected_content = "omega\nalpha 2\n";
    assert_eq!(summary.replacements, 1);
    assert!(summary.wrote);
    assert!(summary.changed);
    assert!(!summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.expected_version, expected_version);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, version_token(expected_content));
    assert_eq!(fs::read_to_string(&file).expect("read"), expected_content);
    Ok(())
}

#[test]
fn apply_search_replace_edit_regex_all_replaces_all_matches() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("regex-all.txt");
    let content = "alpha 1\nalpha 2\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    let summary = apply_search_replace_edit(
        &file,
        Some(&expected_version),
        &EditOperation {
            search: "alpha\\s+\\d".to_string(),
            replacement: "omega".to_string(),
            match_mode: EditMatchMode::Regex,
            occurrence: EditOccurrence::All,
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    let expected_content = "omega\nomega\n";
    assert_eq!(summary.replacements, 2);
    assert!(summary.wrote);
    assert!(summary.changed);
    assert!(!summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.expected_version, expected_version);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, version_token(expected_content));
    assert_eq!(fs::read_to_string(&file).expect("read"), expected_content);
    Ok(())
}

#[test]
fn apply_search_replace_edit_no_match_returns_noop_summary_without_write() -> Result<()> {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("no-match.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = version_token(content);

    let summary = apply_search_replace_edit(
        &file,
        Some(&expected_version),
        &EditOperation {
            search: "gamma".to_string(),
            replacement: "omega".to_string(),
            match_mode: EditMatchMode::Literal,
            occurrence: EditOccurrence::All,
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    assert_eq!(summary.replacements, 0);
    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert!(summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.expected_version, expected_version);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, expected_version);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: diff
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn deterministic_diff_core_replace_has_stable_headers_and_hunks() {
    let path = Path::new("src/file.txt");
    let previous = "alpha\nbeta\ngamma\n";
    let next = "alpha\nomega\ngamma\n";

    let diff = compute_edit_unified_diff_bounded(path, previous, next, DiffBounds::default());

    let expected =
        "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+omega\n gamma\n";
    assert_eq!(diff.text, expected);

    let second = compute_edit_unified_diff_bounded(path, previous, next, DiffBounds::default());
    assert_eq!(diff.text, second.text);
    assert_eq!(
        diff.stats,
        super::diff::DiffStats {
            files_changed: 1,
            insertions: 1,
            deletions: 1
        }
    );
}

#[test]
fn deterministic_diff_core_insert_delete_and_noop_behavior() {
    let path = Path::new("src/file.txt");

    let inserted =
        compute_edit_unified_diff_bounded(path, "alpha\n", "alpha\nbeta\n", DiffBounds::default());
    assert!(inserted.text.contains("+beta\n"));
    assert_eq!(inserted.stats.insertions, 1);
    assert_eq!(inserted.stats.deletions, 0);

    let deleted =
        compute_edit_unified_diff_bounded(path, "alpha\nbeta\n", "alpha\n", DiffBounds::default());
    assert!(deleted.text.contains("-beta\n"));
    assert_eq!(deleted.stats.insertions, 0);
    assert_eq!(deleted.stats.deletions, 1);

    let noop = compute_edit_unified_diff_bounded(path, "alpha\n", "alpha\n", DiffBounds::default());
    assert_eq!(noop.text, "");
    assert_eq!(noop.stats.files_changed, 0);
    assert_eq!(noop.stats.insertions, 0);
    assert_eq!(noop.stats.deletions, 0);
}

#[test]
fn newline_and_eof_semantics_include_no_newline_markers_and_keep_line_endings() {
    let path = Path::new("src/newlines.txt");

    let eof_added =
        compute_edit_unified_diff_bounded(path, "alpha", "alpha\n", DiffBounds::default());
    assert!(eof_added.text.contains("\\ No newline at end of file\n"));

    let eof_removed =
        compute_edit_unified_diff_bounded(path, "alpha\n", "alpha", DiffBounds::default());
    assert!(eof_removed.text.contains("\\ No newline at end of file\n"));

    let crlf_change =
        compute_edit_unified_diff_bounded(path, "a\r\nb\r\n", "a\r\nc\r\n", DiffBounds::default());
    assert!(crlf_change.text.contains("-b\r\n"));
    assert!(crlf_change.text.contains("+c\r\n"));

    let mixed_eol =
        compute_edit_unified_diff_bounded(path, "a\r\nb\n", "a\r\nc\n", DiffBounds::default());
    assert!(mixed_eol.text.contains("-b\n"));
    assert!(mixed_eol.text.contains("+c\n"));
}

#[test]
fn large_diff_bounding_truncates_text_with_markers_but_preserves_summary_stats() {
    let path = Path::new("src/huge.txt");
    let previous = (0..200).map(|i| format!("old-{i}\n")).collect::<String>();
    let next = (0..200).map(|i| format!("new-{i}\n")).collect::<String>();

    let diff = compute_edit_unified_diff_bounded(
        path,
        &previous,
        &next,
        DiffBounds {
            max_bytes: 400,
            max_lines: 30,
        },
    );

    assert!(diff.truncated);
    assert!(diff.text.contains("... diff truncated ..."));
    assert!(diff.text.contains("omitted_lines="));
    assert!(diff.text.contains("omitted_hunks="));
    assert_eq!(diff.omitted_files, 0);
    assert!(diff.text.lines().count() <= 31);
    assert!(diff.text.len() <= 520);
    assert_eq!(diff.stats.files_changed, 1);
    assert_eq!(diff.stats.insertions, 200);
    assert_eq!(diff.stats.deletions, 200);
}

#[test]
fn bounded_diff_never_exceeds_max_bytes_even_for_tiny_budgets() {
    let path = Path::new("src/tiny-budget.txt");
    let previous = (0..64).map(|i| format!("old-{i}\n")).collect::<String>();
    let next = (0..64).map(|i| format!("new-{i}\n")).collect::<String>();

    for max_bytes in [0usize, 1, 2, 3, 4, 5, 8, 13, 21] {
        let diff = compute_edit_unified_diff_bounded(
            path,
            &previous,
            &next,
            DiffBounds {
                max_bytes,
                max_lines: 8,
            },
        );

        assert!(
            diff.text.len() <= max_bytes,
            "max_bytes={max_bytes} produced {} bytes",
            diff.text.len()
        );
        assert!(diff.truncated);
        assert_eq!(diff.stats.files_changed, 1);
        assert_eq!(diff.stats.insertions, 64);
        assert_eq!(diff.stats.deletions, 64);
    }
}

#[test]
fn bounded_diff_uses_deterministic_compact_marker_when_marker_exceeds_budget() {
    let path = Path::new("src/compact-marker.txt");
    let previous = (0..32).map(|i| format!("old-{i}\n")).collect::<String>();
    let next = (0..32).map(|i| format!("new-{i}\n")).collect::<String>();

    let bounds = DiffBounds {
        max_bytes: 4,
        max_lines: 4,
    };
    let first = compute_edit_unified_diff_bounded(path, &previous, &next, bounds);
    let second = compute_edit_unified_diff_bounded(path, &previous, &next, bounds);

    assert_eq!(first.text, "...\n");
    assert_eq!(first.text, second.text);
    assert!(first.truncated);
    assert!(first.omitted_hunks >= 1);
    assert!(first.text.len() <= bounds.max_bytes);
}
