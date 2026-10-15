use super::*;
use crate::bus::Bus;
use crate::protocol::tool_args::CallLine;
use std::io::Write;
use tempfile::tempdir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const TEN_MATCH_LINES: &str = "match line 1\nmatch line 2\nmatch line 3\nmatch line 4\nmatch line 5\n\
     match line 6\nmatch line 7\nmatch line 8\nmatch line 9\nmatch line 10\n";

#[tokio::test]
async fn grep_search_behavior_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        files: Vec<(&'static str, &'static str)>,
        args: serde_json::Value,
        expected_matches: usize,
        expected_total: Option<usize>,
        expected_truncated: bool,
        expected_file_suffix: Option<&'static str>,
        expected_first_line: Option<i64>,
        expected_first_content: Option<&'static str>,
    }

    let cases = vec![
        Case {
            name: "finds a literal match",
            files: vec![("foo.rs", "fn hello_world() {}\n")],
            args: serde_json::json!({"pattern": "hello_world"}),
            expected_matches: 1,
            expected_total: None,
            expected_truncated: false,
            expected_file_suffix: Some("foo.rs"),
            expected_first_line: Some(1),
            expected_first_content: Some("hello_world"),
        },
        Case {
            name: "returns empty when nothing matches",
            files: vec![("foo.rs", "fn unrelated() {}\n")],
            args: serde_json::json!({"pattern": "xyzzy_no_match"}),
            expected_matches: 0,
            expected_total: Some(0),
            expected_truncated: false,
            expected_file_suffix: None,
            expected_first_line: None,
            expected_first_content: None,
        },
        Case {
            name: "case sensitive search misses a different case",
            files: vec![("foo.rs", "Hello World\n")],
            args: serde_json::json!({"pattern": "hello world", "case_insensitive": false}),
            expected_matches: 0,
            expected_total: None,
            expected_truncated: false,
            expected_file_suffix: None,
            expected_first_line: None,
            expected_first_content: None,
        },
        Case {
            name: "case insensitive search matches a different case",
            files: vec![("foo.rs", "Hello World\n")],
            args: serde_json::json!({"pattern": "hello world", "case_insensitive": true}),
            expected_matches: 1,
            expected_total: None,
            expected_truncated: false,
            expected_file_suffix: None,
            expected_first_line: None,
            expected_first_content: None,
        },
        Case {
            name: "glob filter keeps only .rs files",
            files: vec![("foo.rs", "needle\n"), ("foo.txt", "needle\n")],
            args: serde_json::json!({"pattern": "needle", "glob": "*.rs"}),
            expected_matches: 1,
            expected_total: None,
            expected_truncated: false,
            expected_file_suffix: Some(".rs"),
            expected_first_line: None,
            expected_first_content: None,
        },
        Case {
            name: "glob filter keeps only .txt files",
            files: vec![("foo.rs", "needle\n"), ("foo.txt", "needle\n")],
            args: serde_json::json!({"pattern": "needle", "glob": "*.txt"}),
            expected_matches: 1,
            expected_total: None,
            expected_truncated: false,
            expected_file_suffix: Some(".txt"),
            expected_first_line: None,
            expected_first_content: None,
        },
        Case {
            name: "max_results caps the matches and sets truncated",
            files: vec![("foo.rs", TEN_MATCH_LINES)],
            args: serde_json::json!({"pattern": "match line", "max_results": 3}),
            expected_matches: 3,
            expected_total: None,
            expected_truncated: true,
            expected_file_suffix: None,
            expected_first_line: None,
            expected_first_content: None,
        },
        Case {
            name: "max_results above the match count leaves truncated false",
            files: vec![("foo.rs", TEN_MATCH_LINES)],
            args: serde_json::json!({"pattern": "match line", "max_results": 20}),
            expected_matches: 10,
            expected_total: None,
            expected_truncated: false,
            expected_file_suffix: None,
            expected_first_line: None,
            expected_first_content: None,
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempdir()?;
        for (name, content) in &case.files {
            std::fs::write(dir.path().join(name), content)?;
        }
        let mut args = case.args.clone();
        args["path"] = serde_json::json!(dir.path().to_str().ok_or("temp path must be UTF-8")?);

        let result = GrepTool::execute(&args, dir.path(), &Bus::default())
            .await
            .map_err(|e| format!("{e:?}"))?;

        let matches = result["matches"]
            .as_array()
            .ok_or("matches must be an array")?;
        assert_eq!(
            matches.len(),
            case.expected_matches,
            "case '{}': match count",
            case.name
        );
        if let Some(total) = case.expected_total {
            assert_eq!(result["total"], total, "case '{}': total", case.name);
        }
        assert_eq!(
            result["truncated"], case.expected_truncated,
            "case '{}': truncated",
            case.name
        );
        if let Some(suffix) = case.expected_file_suffix {
            let file = matches[0]["file"].as_str().ok_or("file must be a string")?;
            assert!(
                file.ends_with(suffix),
                "case '{}': expected file suffix {suffix}, got {file}",
                case.name
            );
        }
        if let Some(line) = case.expected_first_line {
            assert_eq!(matches[0]["line"], line, "case '{}': line", case.name);
        }
        if let Some(content) = case.expected_first_content {
            let actual = matches[0]["content"]
                .as_str()
                .ok_or("content must be a string")?;
            assert!(
                actual.contains(content),
                "case '{}': expected content to contain {content}, got {actual}",
                case.name
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn grep_invalid_regex_returns_validation_error() {
    let dir = tempdir().unwrap();
    let args = serde_json::json!({"pattern": "[invalid", "path": dir.path().to_str().unwrap()});
    let err = GrepTool::execute(&args, dir.path(), &Bus::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Validation);
    assert!(err.message.contains("Invalid regex pattern"));
}

#[tokio::test]
async fn grep_searches_subdirectories_recursively() -> Result<()> {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("subdir");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("nested.rs"), "deep_match\n").unwrap();

    let args = serde_json::json!({"pattern": "deep_match", "path": dir.path().to_str().unwrap()});
    let result = GrepTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    let matches = result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    let file = matches[0]["file"].as_str().unwrap();
    assert!(
        file.contains("subdir"),
        "expected subdir in path, got: {file}"
    );
    assert!(file.ends_with("nested.rs"));
    Ok(())
}

#[tokio::test]
async fn grep_skips_binary_files_without_error() {
    let dir = tempdir().unwrap();
    let mut f = std::fs::File::create(dir.path().join("binary.bin")).unwrap();
    f.write_all(&[0u8, 1u8, 2u8, 0u8]).unwrap();
    std::fs::write(dir.path().join("normal.rs"), "no match here\n").unwrap();

    let args = serde_json::json!({"pattern": "anything", "path": dir.path().to_str().unwrap()});
    let result = GrepTool::execute(&args, dir.path(), &Bus::default()).await;
    assert!(
        result.is_ok(),
        "should not error on binary files: {result:?}"
    );
}

// === call_line_render ===

#[test]
fn grep_call_line_render_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, &str)> = vec![
        (
            "quoted pattern and glob when path is dot",
            r#"{"pattern":"fn main","path":".","glob":"*.rs"}"#,
            "→ \"fn main\" *.rs",
        ),
        (
            "combines path and glob",
            r#"{"pattern":"fn main","path":"/src","glob":"*.rs"}"#,
            "→ \"fn main\" /src/*.rs",
        ),
        (
            "shows path without glob",
            r#"{"pattern":"fn main","path":"/src"}"#,
            "→ \"fn main\" /src",
        ),
        (
            "shows pattern only without location",
            r#"{"pattern":"fn main"}"#,
            "→ \"fn main\"",
        ),
    ];

    // -- Exec & Check
    for (name, args, expected) in cases {
        assert_eq!(
            GrepTool::call_line_render(args),
            CallLine {
                summary: expected.to_string(),
            },
            "case '{name}'"
        );
    }

    let invalid = "not-json";
    assert_eq!(
        GrepTool::call_line_render(invalid),
        CallLine::from_json_summary(invalid)
    );
    Ok(())
}
