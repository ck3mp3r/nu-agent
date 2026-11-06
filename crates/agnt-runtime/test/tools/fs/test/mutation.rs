use super::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: mutation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn apply_full_content_mutation_table() -> Result<()> {
    // -- Setup & Fixtures
    enum VersionSpec {
        Computed,
        Literal(&'static str),
        Omitted,
    }

    struct Case {
        name: &'static str,
        original: &'static str,
        updated: &'static str,
        version: VersionSpec,
        expected_error: Option<&'static str>,
        expected_wrote: bool,
        expected_changed: bool,
        expected_file_content: &'static str,
    }

    let cases = vec![
        Case {
            name: "matching version writes and reports a deterministic summary",
            original: "line1\nline2\n",
            updated: "line1\nline2\nline3\n",
            version: VersionSpec::Computed,
            expected_error: None,
            expected_wrote: true,
            expected_changed: true,
            expected_file_content: "line1\nline2\nline3\n",
        },
        Case {
            name: "stale version conflicts and does not write",
            original: "before\n",
            updated: "after\n",
            version: VersionSpec::Literal("stale-version"),
            expected_error: Some("conflict"),
            expected_wrote: false,
            expected_changed: false,
            expected_file_content: "before\n",
        },
        Case {
            name: "missing expected version is a validation error",
            original: "seed\n",
            updated: "updated\n",
            version: VersionSpec::Omitted,
            expected_error: Some("validation"),
            expected_wrote: false,
            expected_changed: false,
            expected_file_content: "seed\n",
        },
        Case {
            name: "unchanged content matching the version is deterministic without a write",
            original: "same\ncontent\n",
            updated: "same\ncontent\n",
            version: VersionSpec::Computed,
            expected_error: None,
            expected_wrote: false,
            expected_changed: false,
            expected_file_content: "same\ncontent\n",
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempdir().expect("temp dir");
        let file = dir.path().join("target.txt");
        fs::write(&file, case.original).expect("seed file");

        let expected_version = match case.version {
            VersionSpec::Computed => Some(version_token(case.original)),
            VersionSpec::Literal(version) => Some(version.to_string()),
            VersionSpec::Omitted => None,
        };

        let result = apply_full_content_mutation(&file, expected_version.as_deref(), case.updated);

        match case.expected_error {
            Some("conflict") => {
                let err = result.expect_err("expected conflict");
                assert!(
                    matches!(err, MutateError::Conflict(_)),
                    "case '{}': error kind",
                    case.name
                );
            }
            Some("validation") => {
                let err = result.expect_err("expected validation error");
                assert!(
                    matches!(err, MutateError::MissingExpectedVersion),
                    "case '{}': error kind",
                    case.name
                );
            }
            _ => {
                let summary = result.map_err(|e| format!("{e:?}"))?;
                assert_eq!(
                    summary.wrote, case.expected_wrote,
                    "case '{}': wrote",
                    case.name
                );
                assert_eq!(
                    summary.changed, case.expected_changed,
                    "case '{}': changed",
                    case.name
                );
                assert_eq!(
                    summary.previous_version,
                    version_token(case.original),
                    "case '{}': previous_version",
                    case.name
                );
                assert_eq!(
                    summary.new_version,
                    version_token(case.updated),
                    "case '{}': new_version",
                    case.name
                );
                assert_eq!(
                    summary.previous_bytes,
                    case.original.len(),
                    "case '{}': previous_bytes",
                    case.name
                );
                assert_eq!(
                    summary.new_bytes,
                    case.updated.len(),
                    "case '{}': new_bytes",
                    case.name
                );
                assert_eq!(
                    summary.previous_lines,
                    test_line_count(case.original),
                    "case '{}': previous_lines",
                    case.name
                );
                assert_eq!(
                    summary.new_lines,
                    test_line_count(case.updated),
                    "case '{}': new_lines",
                    case.name
                );
            }
        }

        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            case.expected_file_content,
            "case '{}': file content",
            case.name
        );
    }
    Ok(())
}
