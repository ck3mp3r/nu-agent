use super::core::{
    ConflictError, FileMetadata, atomic_overwrite, check_conflict, metadata_for_path, version_token,
};
use std::fs;
use tempfile::tempdir;

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
fn atomic_overwrite_replaces_file_content() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("target.txt");

    fs::write(&file, "before").expect("seed file");
    atomic_overwrite(&file, b"after").expect("atomic overwrite");

    let actual = fs::read_to_string(&file).expect("read file");
    assert_eq!(actual, "after");
}

#[test]
fn metadata_helper_returns_size_and_mtime() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("meta.txt");

    fs::write(&file, "12345").expect("write file");
    let metadata = metadata_for_path(&file).expect("metadata");

    assert_eq!(
        metadata,
        FileMetadata {
            size_bytes: 5,
            modified_unix_seconds: metadata.modified_unix_seconds,
        }
    );
    assert!(metadata.modified_unix_seconds > 0);
}
