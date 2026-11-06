use super::*;

#[test]
fn plan_create_file_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        seed: Option<&'static str>,
        content: &'static str,
        expected_conflict: bool,
        expected_would_change: bool,
        expected_previous_content: &'static str,
        expected_new_content: Option<&'static str>,
        expected_new_bytes: Option<usize>,
        expected_new_lines: Option<usize>,
    }

    let cases = vec![
        Case {
            name: "nonexistent file",
            seed: None,
            content: "hello world\n",
            expected_conflict: false,
            expected_would_change: true,
            expected_previous_content: "",
            expected_new_content: Some("hello world\n"),
            expected_new_bytes: Some("hello world\n".len()),
            expected_new_lines: Some(1),
        },
        Case {
            name: "existing file conflicts",
            seed: Some("existing\n"),
            content: "new content\n",
            expected_conflict: true,
            expected_would_change: false,
            expected_previous_content: "existing\n",
            expected_new_content: None,
            expected_new_bytes: None,
            expected_new_lines: None,
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempdir().expect("temp dir");
        let file = dir.path().join("target.txt");
        if let Some(seed) = case.seed {
            fs::write(&file, seed).expect("seed");
        }

        let plan = plan_create_file(&file, case.content).map_err(|e| format!("{e:?}"))?;

        assert_eq!(
            plan.conflict, case.expected_conflict,
            "case '{}': conflict",
            case.name
        );
        assert_eq!(
            plan.would_change, case.expected_would_change,
            "case '{}': would_change",
            case.name
        );
        assert!(!plan.noop, "case '{}': noop", case.name);
        assert_eq!(plan.replacements, 0, "case '{}': replacements", case.name);
        assert_eq!(
            plan.previous_content, case.expected_previous_content,
            "case '{}': previous_content",
            case.name
        );
        assert_eq!(
            plan.previous_bytes,
            case.seed.map_or(0, str::len),
            "case '{}': previous_bytes",
            case.name
        );
        assert_eq!(
            plan.previous_lines,
            case.seed.map_or(0, test_line_count),
            "case '{}': previous_lines",
            case.name
        );
        assert_eq!(
            plan.previous_version,
            version_token(case.seed.unwrap_or("")),
            "case '{}': previous_version",
            case.name
        );
        if let Some(expected) = case.expected_new_content {
            assert_eq!(
                plan.new_content, expected,
                "case '{}': new_content",
                case.name
            );
        }
        if let Some(expected) = case.expected_new_bytes {
            assert_eq!(plan.new_bytes, expected, "case '{}': new_bytes", case.name);
        }
        if let Some(expected) = case.expected_new_lines {
            assert_eq!(plan.new_lines, expected, "case '{}': new_lines", case.name);
        }
    }
    Ok(())
}

#[test]
fn apply_create_file_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        seed: Option<&'static str>,
        content: &'static str,
        expected_wrote: bool,
        expected_changed: bool,
        expected_conflict: bool,
        expected_file_content: &'static str,
    }

    let cases = vec![
        Case {
            name: "creates a new file atomically",
            seed: None,
            content: "brand new file\nline two\n",
            expected_wrote: true,
            expected_changed: true,
            expected_conflict: false,
            expected_file_content: "brand new file\nline two\n",
        },
        Case {
            name: "conflicts when the file appears between plan and apply",
            seed: Some("someone else created it\n"),
            content: "intended content\n",
            expected_wrote: false,
            expected_changed: false,
            expected_conflict: true,
            expected_file_content: "someone else created it\n",
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempdir().expect("temp dir");
        let file = dir.path().join("target.txt");
        if let Some(seed) = case.seed {
            fs::write(&file, seed).expect("seed");
        }

        let summary = apply_create_file(&file, case.content).map_err(|e| format!("{e:?}"))?;

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
            summary.conflict, case.expected_conflict,
            "case '{}': conflict",
            case.name
        );
        assert!(!summary.noop, "case '{}': noop", case.name);
        assert_eq!(
            summary.replacements, 0,
            "case '{}': replacements",
            case.name
        );
        assert_eq!(
            summary.previous_version,
            version_token(case.seed.unwrap_or("")),
            "case '{}': previous_version",
            case.name
        );
        assert_eq!(
            summary.new_version,
            version_token(case.expected_file_content),
            "case '{}': new_version",
            case.name
        );
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            case.expected_file_content,
            "case '{}': file content",
            case.name
        );
    }
    Ok(())
}
