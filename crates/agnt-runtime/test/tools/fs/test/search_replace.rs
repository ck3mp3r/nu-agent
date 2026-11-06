use super::*;

#[test]
fn apply_search_replace_edit_mode_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        content: &'static str,
        search: &'static str,
        replacement: &'static str,
        match_mode: EditMatchMode,
        occurrence: EditOccurrence,
        expected_content: &'static str,
        expected_replacements: usize,
        expected_wrote: bool,
        expected_changed: bool,
        expected_noop: bool,
    }

    let cases = vec![
        Case {
            name: "literal first replaces only the first match",
            content: "alpha beta alpha\n",
            search: "alpha",
            replacement: "omega",
            match_mode: EditMatchMode::Literal,
            occurrence: EditOccurrence::First,
            expected_content: "omega beta alpha\n",
            expected_replacements: 1,
            expected_wrote: true,
            expected_changed: true,
            expected_noop: false,
        },
        Case {
            name: "literal all replaces all matches",
            content: "alpha beta alpha\n",
            search: "alpha",
            replacement: "omega",
            match_mode: EditMatchMode::Literal,
            occurrence: EditOccurrence::All,
            expected_content: "omega beta omega\n",
            expected_replacements: 2,
            expected_wrote: true,
            expected_changed: true,
            expected_noop: false,
        },
        Case {
            name: "regex first replaces only the first match",
            content: "alpha 1\nalpha 2\n",
            search: "alpha\\s+\\d",
            replacement: "omega",
            match_mode: EditMatchMode::Regex,
            occurrence: EditOccurrence::First,
            expected_content: "omega\nalpha 2\n",
            expected_replacements: 1,
            expected_wrote: true,
            expected_changed: true,
            expected_noop: false,
        },
        Case {
            name: "regex all replaces all matches",
            content: "alpha 1\nalpha 2\n",
            search: "alpha\\s+\\d",
            replacement: "omega",
            match_mode: EditMatchMode::Regex,
            occurrence: EditOccurrence::All,
            expected_content: "omega\nomega\n",
            expected_replacements: 2,
            expected_wrote: true,
            expected_changed: true,
            expected_noop: false,
        },
        Case {
            name: "no match returns a noop summary without write",
            content: "alpha beta\n",
            search: "gamma",
            replacement: "omega",
            match_mode: EditMatchMode::Literal,
            occurrence: EditOccurrence::All,
            expected_content: "alpha beta\n",
            expected_replacements: 0,
            expected_wrote: false,
            expected_changed: false,
            expected_noop: true,
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempdir().expect("temp dir");
        let file = dir.path().join("target.txt");
        fs::write(&file, case.content).expect("seed file");
        let expected_version = version_token(case.content);

        let summary = apply_search_replace_edit(
            &file,
            Some(&expected_version),
            &EditOperation {
                search: case.search.to_string(),
                replacement: case.replacement.to_string(),
                match_mode: case.match_mode,
                occurrence: case.occurrence,
            },
        )
        .map_err(|e| format!("{e:?}"))?;

        assert_eq!(
            summary.replacements, case.expected_replacements,
            "case '{}': replacements",
            case.name
        );
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
            summary.noop, case.expected_noop,
            "case '{}': noop",
            case.name
        );
        assert!(!summary.conflict, "case '{}': conflict", case.name);
        assert_eq!(
            summary.expected_version, expected_version,
            "case '{}': expected_version",
            case.name
        );
        assert_eq!(
            summary.previous_version, expected_version,
            "case '{}': previous_version",
            case.name
        );
        assert_eq!(
            summary.new_version,
            version_token(case.expected_content),
            "case '{}': new_version",
            case.name
        );
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            case.expected_content,
            "case '{}': file content",
            case.name
        );
    }
    Ok(())
}
