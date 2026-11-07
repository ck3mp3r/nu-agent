use super::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: edit
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn apply_search_replace_edit_validation_table() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempdir().expect("temp dir");
    let missing_version = dir.path().join("missing-version.txt");
    fs::write(&missing_version, "alpha beta\n").expect("seed file");
    let stale = dir.path().join("conflict.txt");
    let stale_content = "alpha beta alpha\n";
    fs::write(&stale, stale_content).expect("seed file");

    let operation = EditOperation {
        search: "alpha".to_string(),
        replacement: "omega".to_string(),
        match_mode: EditMatchMode::Literal,
        occurrence: EditOccurrence::All,
    };

    // -- Exec & Check: a missing expected version is a validation error
    let err = apply_search_replace_edit(&missing_version, None, &operation)
        .expect_err("expected validation error");
    assert!(matches!(err, MutateError::MissingExpectedVersion));
    assert_eq!(
        fs::read_to_string(&missing_version).expect("read"),
        "alpha beta\n"
    );

    // -- Exec & Check: a stale version conflicts without writing
    let summary = apply_search_replace_edit(&stale, Some("stale-version"), &operation)
        .map_err(|e| format!("{e:?}"))?;

    let current = version_token(stale_content);
    assert_eq!(summary.replacements, 0);
    assert!(!summary.wrote);
    assert!(!summary.changed);
    assert!(!summary.noop);
    assert!(summary.conflict);
    assert_eq!(summary.expected_version, "stale-version");
    assert_eq!(summary.previous_version, current);
    assert_eq!(summary.new_version, current);
    assert_eq!(fs::read_to_string(&stale).expect("read"), stale_content);
    Ok(())
}
