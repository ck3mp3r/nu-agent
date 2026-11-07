use super::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: core
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn version_token_and_check_conflict_are_deterministic() {
    // -- Setup & Fixtures
    let current = "abc123".to_string();

    // -- Exec & Check
    let a1 = version_token("hello");
    let a2 = version_token("hello");
    let b = version_token("hello world");
    assert_eq!(
        a1, a2,
        "the same content must produce the same version token"
    );
    assert_ne!(
        a1, b,
        "different content must produce different version tokens"
    );

    assert!(
        check_conflict(None, &current).is_ok(),
        "an absent expected version must not conflict"
    );
    assert!(
        check_conflict(Some("abc123"), &current).is_ok(),
        "a matching expected version must not conflict"
    );

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
fn atomic_overwrite_and_metadata_helpers() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("target.txt");
    fs::write(&file, "before").expect("seed file");

    // -- Exec
    atomic_overwrite(&file, b"after").map_err(|e| format!("{e:?}"))?;
    let metadata = metadata_for_path(&file).map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(fs::read_to_string(&file).expect("read file"), "after");
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
