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
        expected_files_changed: Option<usize>,
    }

    let cases = vec![
        Case {
            name: "create operation returns a diff display",
            seed: None,
            mode: None,
            operation: serde_json::json!({"type": "create", "content": "hello\n"}),
            expected_diff: ("+hello", "+hello"),
            expected_files_changed: Some(1),
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
            expected_files_changed: None,
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

        let display = EditTool::preview(&args, dir.path())
            .ok_or("preview must produce a display for a valid plan")?;
        assert_eq!(display.title, None, "case '{}': title", case.name);
        assert_eq!(
            display.sections.len(),
            1,
            "case '{}': section count",
            case.name
        );
        let section = &display.sections[0];
        assert!(
            matches!(section.kind, ContentKind::Diff { .. }),
            "case '{}': section kind must be Diff, got {:?}",
            case.name,
            section.kind
        );
        let (removed, added) = case.expected_diff;
        assert!(
            section.content.contains(removed) && section.content.contains(added),
            "case '{}': diff must contain {removed} and {added}, got: {:?}",
            case.name,
            section.content
        );
        if let Some(files_changed) = case.expected_files_changed {
            let stats = section
                .stats
                .clone()
                .ok_or("diff section must carry stats")?;
            assert_eq!(
                stats.files_changed,
                Some(files_changed),
                "case '{}': files_changed",
                case.name
            );
        }
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
