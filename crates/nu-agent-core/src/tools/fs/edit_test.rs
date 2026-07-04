use super::core::{
    EditMatchMode, EditOccurrence, EditOperation, MutateError, apply_create_file,
    apply_search_replace_edit, plan_create_file, version_token,
};
use std::fs;
use tempfile::tempdir;

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
fn apply_search_replace_edit_conflict_on_stale_version_without_write() {
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
    .expect("conflict summary");

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
}

#[test]
fn plan_create_file_returns_plan_for_nonexistent_file() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("new-create.txt");
    let content = "hello world\n";

    let plan = plan_create_file(&file, content).expect("plan");

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
}

#[test]
fn plan_create_file_conflicts_when_file_already_exists() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("exists.txt");
    fs::write(&file, "existing\n").expect("seed");

    let plan = plan_create_file(&file, "new content\n").expect("plan");

    assert!(plan.conflict);
    assert!(!plan.would_change);
    assert!(!plan.noop);
    assert_eq!(plan.previous_version, version_token("existing\n"));
}

#[test]
fn apply_create_file_creates_new_file_atomically() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("created-by-apply.txt");
    let content = "brand new file\nline two\n";

    let summary = apply_create_file(&file, content).expect("apply");

    assert!(summary.wrote);
    assert!(summary.changed);
    assert!(!summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.replacements, 0);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
    assert_eq!(summary.previous_version, version_token(""));
    assert_eq!(summary.new_version, version_token(content));
}

#[test]
fn apply_create_file_conflicts_when_file_appears_between_plan_and_apply() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("race-condition.txt");
    let content = "intended content\n";

    fs::write(&file, "someone else created it\n").expect("seed");

    let summary = apply_create_file(&file, content).expect("summary");

    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert!(summary.conflict);
    assert_eq!(
        fs::read_to_string(&file).expect("read"),
        "someone else created it\n"
    );
}

#[test]
fn apply_search_replace_edit_literal_first_replaces_only_first_match() {
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
    .expect("apply");

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
}

#[test]
fn apply_search_replace_edit_literal_all_replaces_all_matches() {
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
    .expect("apply");

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
}

#[test]
fn apply_search_replace_edit_regex_first_replaces_only_first_match() {
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
    .expect("apply");

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
}

#[test]
fn apply_search_replace_edit_regex_all_replaces_all_matches() {
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
    .expect("apply");

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
}

#[test]
fn apply_search_replace_edit_no_match_returns_noop_summary_without_write() {
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
    .expect("apply");

    assert_eq!(summary.replacements, 0);
    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert!(summary.noop);
    assert!(!summary.conflict);
    assert_eq!(summary.expected_version, expected_version);
    assert_eq!(summary.previous_version, expected_version);
    assert_eq!(summary.new_version, expected_version);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
}
