use super::*;

#[tokio::test]
async fn edit_apply_and_preview_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        seed: Option<&'static str>,
        mode: Option<&'static str>,
        operation: serde_json::Value,
        expected_applied: bool,
        expected_wrote: bool,
        expected_would_change: Option<bool>,
        expected_diff_contains: Option<&'static str>,
        expected_content: &'static str,
    }

    let cases = vec![
        Case {
            name: "apply search_replace modifies the file",
            seed: Some("hello world\n"),
            mode: None,
            operation: serde_json::json!({
                "type": "search_replace",
                "search": "world",
                "replacement": "there"
            }),
            expected_applied: true,
            expected_wrote: true,
            expected_would_change: Some(true),
            expected_diff_contains: None,
            expected_content: "hello there\n",
        },
        Case {
            name: "preview returns a diff without writing",
            seed: Some("hello world\n"),
            mode: Some("preview"),
            operation: serde_json::json!({
                "type": "search_replace",
                "search": "world",
                "replacement": "there"
            }),
            expected_applied: false,
            expected_wrote: false,
            expected_would_change: Some(true),
            expected_diff_contains: Some("world"),
            expected_content: "hello world\n",
        },
        Case {
            name: "create writes a new file",
            seed: None,
            mode: None,
            operation: serde_json::json!({"type": "create", "content": "new content\n"}),
            expected_applied: true,
            expected_wrote: true,
            expected_would_change: None,
            expected_diff_contains: None,
            expected_content: "new content\n",
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("test.txt");
        if let Some(seed) = case.seed {
            std::fs::write(&path, seed)?;
        }
        let mut args = serde_json::json!({
            "path": path.to_str().ok_or("temp path must be UTF-8")?,
            "operation": case.operation,
        });
        if let Some(mode) = case.mode {
            args["mode"] = serde_json::json!(mode);
        }
        if let Some(seed) = case.seed {
            args["expected_version"] =
                serde_json::json!(crate::tools::fs::core::version_token(seed));
        }

        let result = EditTool::execute(&args, dir.path(), &Bus::default())
            .await
            .map_err(|e| format!("{e:?}"))?;

        assert_eq!(
            result["applied"], case.expected_applied,
            "case '{}': applied",
            case.name
        );
        assert_eq!(
            result["wrote"], case.expected_wrote,
            "case '{}': wrote",
            case.name
        );
        if let Some(would_change) = case.expected_would_change {
            assert_eq!(
                result["would_change"], would_change,
                "case '{}': would_change",
                case.name
            );
        }
        if let Some(fragment) = case.expected_diff_contains {
            let diff = result["diff"].as_str().ok_or("diff must be a string")?;
            assert!(
                diff.contains(fragment),
                "case '{}': diff must contain {fragment}, got {diff}",
                case.name
            );
        }
        assert_eq!(
            std::fs::read_to_string(&path)?,
            case.expected_content,
            "case '{}': file content",
            case.name
        );
    }
    Ok(())
}

#[tokio::test]
async fn edit_error_table() -> Result<()> {
    // -- Setup & Fixtures
    enum VersionSpec {
        Computed,
        Literal(&'static str),
        Omitted,
    }

    struct Case {
        name: &'static str,
        seed: &'static str,
        version: VersionSpec,
        operation: serde_json::Value,
        expected_message: &'static str,
        expected_details: Vec<(&'static str, serde_json::Value)>,
    }

    let cases = vec![
        Case {
            name: "stale version conflicts",
            seed: "hello world\n",
            version: VersionSpec::Literal("wrong-version"),
            operation: serde_json::json!({
                "type": "search_replace",
                "search": "world",
                "replacement": "there"
            }),
            expected_message: "conflict",
            expected_details: vec![
                ("conflict", serde_json::json!(true)),
                ("applied", serde_json::json!(false)),
                ("diff", serde_json::json!("")),
            ],
        },
        Case {
            name: "noop when the replacement equals the search",
            seed: "hello world\n",
            version: VersionSpec::Computed,
            operation: serde_json::json!({
                "type": "search_replace",
                "search": "world",
                "replacement": "world"
            }),
            expected_message: "no change",
            expected_details: vec![
                ("noop", serde_json::json!(true)),
                ("applied", serde_json::json!(false)),
                ("changed", serde_json::json!(false)),
            ],
        },
        Case {
            name: "search not found is a no-op",
            seed: "hello world\n",
            version: VersionSpec::Computed,
            operation: serde_json::json!({
                "type": "search_replace",
                "search": "absent-token",
                "replacement": "there"
            }),
            expected_message: "no change",
            expected_details: vec![
                ("noop", serde_json::json!(true)),
                ("replacements", serde_json::json!(0)),
                ("diff", serde_json::json!("")),
            ],
        },
        Case {
            name: "create on an existing file conflicts",
            seed: "already here\n",
            version: VersionSpec::Omitted,
            operation: serde_json::json!({"type": "create", "content": "new content\n"}),
            expected_message: "already exists",
            expected_details: vec![
                ("conflict", serde_json::json!(true)),
                ("applied", serde_json::json!(false)),
            ],
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("test.txt");
        std::fs::write(&path, case.seed)?;
        let mut args = serde_json::json!({
            "path": path.to_str().ok_or("temp path must be UTF-8")?,
            "operation": case.operation,
        });
        match case.version {
            VersionSpec::Computed => {
                args["expected_version"] =
                    serde_json::json!(crate::tools::fs::core::version_token(case.seed));
            }
            VersionSpec::Literal(version) => {
                args["expected_version"] = serde_json::json!(version);
            }
            VersionSpec::Omitted => {}
        }

        let error = EditTool::execute(&args, dir.path(), &Bus::default())
            .await
            .err()
            .ok_or("this edit must fail the tool call")?;

        assert!(
            error.message.contains(case.expected_message),
            "case '{}': message must contain '{}', got: {}",
            case.name,
            case.expected_message,
            error.message
        );
        let details = error
            .details
            .ok_or("error must carry the contract response")?;
        for (field, expected) in &case.expected_details {
            assert_eq!(
                details.get(*field),
                Some(expected),
                "case '{}': details[{field}]",
                case.name
            );
        }
        assert_eq!(
            std::fs::read_to_string(&path)?,
            case.seed,
            "case '{}': file must be untouched",
            case.name
        );
    }
    Ok(())
}

#[tokio::test]
async fn edit_missing_file_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("nonexistent.txt");
    let args = serde_json::json!({
        "path": path.to_str().ok_or("temp path must be UTF-8")?,
        "expected_version": "some-version",
        "operation": {
            "type": "search_replace",
            "search": "foo",
            "replacement": "bar"
        }
    });

    // -- Exec
    let result = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;

    // -- Check
    assert_eq!(result["applied"], false);
    assert_eq!(result["would_change"], false);
    let diagnostics = result["diagnostics"]
        .as_array()
        .ok_or("diagnostics must be an array")?;
    let diagnostic = diagnostics
        .first()
        .ok_or("a missing file must produce a diagnostic")?;
    let message = diagnostic["message"]
        .as_str()
        .ok_or("diagnostic message must be a string")?;
    assert!(
        message.contains("No such file or directory") || message.contains("not found"),
        "the diagnostic must name the missing file, got: {message}"
    );
    assert_eq!(diagnostic["class"], "internal");
    assert!(!path.exists(), "a rejected edit must not create the file");
    Ok(())
}

#[tokio::test]
async fn edit_json_shape_preserved() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world\n").unwrap();
    let version = crate::tools::fs::core::version_token("hello world\n");
    let args = serde_json::json!({
        "path": path.to_str().unwrap(),
        "expected_version": version,
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "there"
        }
    });
    let result = EditTool::execute(&args, dir.path(), &Bus::default())
        .await
        .map_err(|e| format!("{e:?}"))?;
    for field in [
        "path",
        "mode",
        "applied",
        "would_change",
        "diff",
        "stats",
        "diagnostics",
        "changed",
        "wrote",
        "noop",
        "conflict",
    ] {
        assert!(result.get(field).is_some(), "missing field: {field}");
    }
    Ok(())
}
