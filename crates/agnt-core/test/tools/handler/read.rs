use super::*;
use crate::bus::Bus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn read_execute_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        content: &'static str,
        args: serde_json::Value,
        expected_total_lines: i64,
        expected_present: Vec<&'static str>,
        expected_absent: Vec<&'static str>,
    }

    let cases = vec![
        Case {
            name: "returns file content",
            content: "hello world\n",
            args: serde_json::json!({}),
            expected_total_lines: 1,
            expected_present: vec!["hello world"],
            expected_absent: vec![],
        },
        Case {
            name: "returns total lines",
            content: "line1\nline2\nline3\n",
            args: serde_json::json!({}),
            expected_total_lines: 3,
            expected_present: vec!["line1", "line2", "line3"],
            expected_absent: vec![],
        },
        Case {
            name: "respects offset",
            content: "line1\nline2\nline3\nline4\nline5\n",
            args: serde_json::json!({"offset": 2, "limit": 3}),
            expected_total_lines: 5,
            expected_present: vec!["line3"],
            expected_absent: vec!["line1"],
        },
        Case {
            name: "respects limit",
            content: "line1\nline2\nline3\nline4\nline5\n",
            args: serde_json::json!({"offset": 0, "limit": 3}),
            expected_total_lines: 5,
            expected_present: vec!["line1", "line3"],
            expected_absent: vec!["line4"],
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("test.txt");
        std::fs::write(&path, case.content)?;
        let mut args = case.args.clone();
        args["path"] = serde_json::json!(path.to_str().ok_or("temp path must be UTF-8")?);

        let result = ReadTool::execute(&args, dir.path(), &Bus::default())
            .await
            .map_err(|e| format!("{e:?}"))?;

        let content = result["content"]
            .as_str()
            .ok_or("content must be a string")?;
        assert_eq!(
            result["total_lines"], case.expected_total_lines,
            "case '{}': total_lines",
            case.name
        );
        for fragment in &case.expected_present {
            assert!(
                content.contains(fragment),
                "case '{}': content must contain {fragment}, got {content}",
                case.name
            );
        }
        for fragment in &case.expected_absent {
            assert!(
                !content.contains(fragment),
                "case '{}': content must not contain {fragment}, got {content}",
                case.name
            );
        }
        assert!(
            result["version"].as_str().is_some_and(|v| !v.is_empty()),
            "case '{}': version must be non-empty",
            case.name
        );
    }
    Ok(())
}

#[tokio::test]
async fn read_missing_file_returns_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let args = serde_json::json!({ "path": "/nonexistent/path/file.txt" });
    let result = ReadTool::execute(&args, dir.path(), &Bus::default()).await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().kind,
        super::super::ToolErrorKind::Runtime
    );
}

// === call_line_render ===
