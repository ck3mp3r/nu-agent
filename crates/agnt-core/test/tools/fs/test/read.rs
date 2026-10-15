use super::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Tests: read
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn read_file_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        content: &'static str,
        offset: Option<usize>,
        limit: Option<usize>,
        expected_content: &'static str,
        expected_total_lines: usize,
    }

    let cases = vec![
        Case {
            name: "full content and metadata",
            content: "line1\nline2\nline3\n",
            offset: None,
            limit: None,
            expected_content: "line1\nline2\nline3\n",
            expected_total_lines: 3,
        },
        Case {
            name: "window content and metadata",
            content: "a\nb\nc\nd\n",
            offset: Some(1),
            limit: Some(2),
            expected_content: "b\nc\n",
            expected_total_lines: 4,
        },
        Case {
            name: "empty file is deterministic",
            content: "",
            offset: None,
            limit: None,
            expected_content: "",
            expected_total_lines: 0,
        },
        Case {
            name: "offset beyond eof returns an empty window",
            content: "x\ny\n",
            offset: Some(10),
            limit: Some(3),
            expected_content: "",
            expected_total_lines: 2,
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempdir().expect("temp dir");
        let file = dir.path().join("sample.txt");
        fs::write(&file, case.content).expect("write file");

        let response = read_file(
            &file,
            ReadRequest {
                offset: case.offset,
                limit: case.limit,
            },
        )
        .map_err(|e| format!("{e:?}"))?;

        assert_eq!(
            response.content, case.expected_content,
            "case '{}': content",
            case.name
        );
        assert_eq!(
            response.total_lines, case.expected_total_lines,
            "case '{}': total_lines",
            case.name
        );
        assert_eq!(response.offset, case.offset, "case '{}': offset", case.name);
        assert_eq!(response.limit, case.limit, "case '{}': limit", case.name);
        assert!(
            !response.version.is_empty(),
            "case '{}': version must be non-empty",
            case.name
        );
    }
    Ok(())
}
