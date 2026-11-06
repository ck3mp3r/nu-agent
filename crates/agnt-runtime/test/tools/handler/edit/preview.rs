use super::*;

// === Previewable ===

#[test]
fn edit_preview_diff_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        seed: Option<&'static str>,
        mode: Option<&'static str>,
        operation: serde_json::Value,
        expected_diff: (&'static str, &'static str),
    }

    let cases = vec![
        Case {
            name: "create operation returns a diff display",
            seed: None,
            mode: None,
            operation: serde_json::json!({"type": "create", "content": "hello\n"}),
            expected_diff: ("+hello", "+hello"),
        },
        Case {
            name: "search_replace returns a diff display",
            seed: Some("hello\nworld\n"),
            mode: Some("apply"),
            operation: serde_json::json!({
                "type": "search_replace",
                "search": "world",
                "replacement": "there"
            }),
            expected_diff: ("-world", "+there"),
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("target.txt");
        if let Some(seed) = case.seed {
            std::fs::write(&path, seed)?;
        }
        let mut args = serde_json::json!({
            "path": path.to_string_lossy(),
            "operation": case.operation,
        });
        if let Some(mode) = case.mode {
            args["mode"] = serde_json::json!(mode);
        }

        let diff_text = EditTool::preview(&args, dir.path())
            .ok_or("preview must produce the proposed diff for a valid plan")?;
        let (removed, added) = case.expected_diff;
        assert!(
            diff_text.contains(removed) && diff_text.contains(added),
            "case '{}': diff must contain {removed} and {added}, got: {diff_text:?}",
            case.name
        );
    }
    Ok(())
}

#[test]
fn edit_preview_none_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        seed: Option<&'static str>,
        build_args: fn(&std::path::Path) -> serde_json::Value,
    }

    let cases = vec![
        Case {
            name: "invalid args",
            seed: None,
            build_args: |_path| serde_json::json!({"path": "nowhere.txt"}),
        },
        Case {
            name: "noop",
            seed: Some("hello\nworld\n"),
            build_args: |path| {
                serde_json::json!({
                    "path": path.to_string_lossy(),
                    "mode": "apply",
                    "operation": {
                        "type": "search_replace",
                        "search": "world",
                        "replacement": "world"
                    }
                })
            },
        },
        Case {
            name: "search not found",
            seed: Some("hello\nworld\n"),
            build_args: |path| {
                serde_json::json!({
                    "path": path.to_string_lossy(),
                    "mode": "apply",
                    "operation": {
                        "type": "search_replace",
                        "search": "absent-token",
                        "replacement": "there"
                    }
                })
            },
        },
        Case {
            name: "stale version",
            seed: Some("hello\nworld\n"),
            build_args: |path| {
                serde_json::json!({
                    "path": path.to_string_lossy(),
                    "mode": "apply",
                    "expected_version": "deadbeef",
                    "operation": {
                        "type": "search_replace",
                        "search": "world",
                        "replacement": "there"
                    }
                })
            },
        },
        Case {
            name: "create existing file",
            seed: Some("already here\n"),
            build_args: |path| {
                serde_json::json!({
                    "path": path.to_string_lossy(),
                    "operation": {"type": "create", "content": "new content\n"}
                })
            },
        },
    ];

    // -- Exec & Check
    for case in cases {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("target.txt");
        if let Some(seed) = case.seed {
            std::fs::write(&path, seed)?;
        }
        let args = (case.build_args)(&path);
        assert!(
            EditTool::preview(&args, dir.path()).is_none(),
            "case '{}': preview must return None",
            case.name
        );
    }
    Ok(())
}
