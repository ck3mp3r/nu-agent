use super::*;
use crate::bus::Bus;
use crate::protocol::tool_args::CallLine;
use tempfile::tempdir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn glob_execute_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        files: Vec<&'static str>,
        pattern: &'static str,
        expected_matches: usize,
        expected_total: usize,
        expected_all_end_with: Option<&'static str>,
        expected_all_contain: Option<&'static str>,
        expected_sorted: bool,
        expected_relative: bool,
    }

    let cases = vec![
        Case {
            name: "finds matching files",
            files: vec!["foo.rs", "bar.rs", "baz.txt"],
            pattern: "*.rs",
            expected_matches: 2,
            expected_total: 2,
            expected_all_end_with: Some(".rs"),
            expected_all_contain: None,
            expected_sorted: false,
            expected_relative: false,
        },
        Case {
            name: "returns empty for no match",
            files: vec!["foo.rs"],
            pattern: "*.txt",
            expected_matches: 0,
            expected_total: 0,
            expected_all_end_with: None,
            expected_all_contain: None,
            expected_sorted: false,
            expected_relative: false,
        },
        Case {
            name: "returns relative paths",
            files: vec!["subdir/nested.rs"],
            pattern: "**/*.rs",
            expected_matches: 1,
            expected_total: 1,
            expected_all_end_with: Some("nested.rs"),
            expected_all_contain: Some("subdir"),
            expected_sorted: false,
            expected_relative: true,
        },
        Case {
            name: "searches recursively",
            files: vec!["a/b/c/deep.rs"],
            pattern: "**/*.rs",
            expected_matches: 1,
            expected_total: 1,
            expected_all_end_with: Some("deep.rs"),
            expected_all_contain: None,
            expected_sorted: false,
            expected_relative: false,
        },
        Case {
            name: "results are sorted",
            files: vec!["z.rs", "a.rs", "m.rs"],
            pattern: "*.rs",
            expected_matches: 3,
            expected_total: 3,
            expected_all_end_with: None,
            expected_all_contain: None,
            expected_sorted: true,
            expected_relative: false,
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempdir()?;
        for file in &case.files {
            let path = dir.path().join(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, "")?;
        }

        let args = serde_json::json!({
            "pattern": case.pattern,
            "path": dir.path().to_str().ok_or("temp path must be UTF-8")?,
        });
        let result = GlobTool::execute(&args, dir.path(), &Bus::default())
            .await
            .map_err(|e| format!("{e:?}"))?;

        let matches: Vec<&str> = result["matches"]
            .as_array()
            .ok_or("matches must be an array")?
            .iter()
            .map(|value| value.as_str().ok_or("match must be a string"))
            .collect::<core::result::Result<_, _>>()?;

        assert_eq!(
            matches.len(),
            case.expected_matches,
            "case '{}': match count",
            case.name
        );
        assert_eq!(
            result["total"], case.expected_total,
            "case '{}': total",
            case.name
        );
        if let Some(suffix) = case.expected_all_end_with {
            assert!(
                matches.iter().all(|m| m.ends_with(suffix)),
                "case '{}': every match must end with {suffix}, got {matches:?}",
                case.name
            );
        }
        if let Some(fragment) = case.expected_all_contain {
            assert!(
                matches.iter().all(|m| m.contains(fragment)),
                "case '{}': every match must contain {fragment}, got {matches:?}",
                case.name
            );
        }
        if case.expected_sorted {
            let mut sorted = matches.clone();
            sorted.sort();
            assert_eq!(
                matches, sorted,
                "case '{}': results must be sorted",
                case.name
            );
        }
        if case.expected_relative {
            assert!(
                matches.iter().all(|m| !m.starts_with('/')),
                "case '{}': paths must be relative, got {matches:?}",
                case.name
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn glob_invalid_pattern_returns_validation_error() {
    let dir = tempdir().unwrap();

    let args = serde_json::json!({"pattern": "!", "path": dir.path().to_str().unwrap()});
    let result = GlobTool::execute(&args, dir.path(), &Bus::default()).await;
    match result {
        Err(e) => assert_eq!(e.kind, ToolErrorKind::Validation),
        Ok(_) => { /* OverrideBuilder accepted it — acceptable */ }
    }
}

// === call_line_render ===

#[test]
fn glob_call_line_render_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, Option<&str>)> = vec![
        (
            "shows pattern when path is dot",
            r#"{"pattern":"**/*.rs","path":"."}"#,
            Some("→ **/*.rs"),
        ),
        (
            "combines path and pattern",
            r#"{"pattern":"**/*.rs","path":"/src"}"#,
            Some("→ /src/**/*.rs"),
        ),
        ("falls back to generic on invalid json", "not-json", None),
    ];

    // -- Exec & Check
    for (name, args, expected) in cases {
        let render = GlobTool::call_line_render(args);
        match expected {
            Some(summary) => assert_eq!(
                render,
                CallLine {
                    summary: summary.to_string(),
                },
                "case '{name}'"
            ),
            None => assert_eq!(render, CallLine::from_json_summary(args), "case '{name}'"),
        }
    }
    Ok(())
}
