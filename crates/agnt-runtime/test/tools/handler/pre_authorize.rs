use serde_json::json;

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_pre_authorize_fs_tool_nu_carries_no_producer_preview() -> Result<()> {
    // -- Setup & Fixtures: a nu call whose command is in the arguments. The
    // code block a gate card shows is derived by the surface from that
    // command, so the producer owes no preview for it. A nu call without a
    // non-empty command has no card at all.
    let cases: Vec<(&str, serde_json::Value, bool)> = vec![
        (
            "string command",
            json!({"command": "ls | where size > 1mb"}),
            true,
        ),
        ("missing command", json!({}), false),
        ("non-string command", json!({"command": 42}), false),
    ];

    // -- Exec & Check
    for (name, arguments, has_card) in cases {
        let args = arguments
            .as_object()
            .ok_or("case arguments must be an object")?;
        let output =
            pre_authorize_fs_tool(Some(BuiltinKind::Nu), args, std::path::Path::new("/tmp"));

        if !has_card {
            assert!(
                output.is_none(),
                "case '{name}': a nu call without a command has no card"
            );
            continue;
        }
        let output = output.ok_or("a nu call with a command must carry a card")?;
        assert!(
            output.preview.is_none(),
            "case '{name}': nu carries no producer preview"
        );
        assert!(
            output.ask_context.preview.is_none(),
            "case '{name}': the ask context carries none either"
        );
    }
    Ok(())
}

#[test]
fn test_pre_authorize_fs_tool_edit_path_unchanged_missing_command() -> Result<()> {
    // The edit path returns None when the arguments do not form a valid
    // apply plan. These arguments fail validation (no operation), so the
    // unchanged None path runs without touching disk.
    // -- Setup & Fixtures
    let arguments = json!({"path": "nonexistent-file.txt"});
    let args = arguments
        .as_object()
        .ok_or("edit arguments must be an object")?;

    // -- Exec
    let output = pre_authorize_fs_tool(Some(BuiltinKind::Edit), args, std::path::Path::new("/tmp"));

    // -- Check
    assert!(
        output.is_none(),
        "edit path behavior must stay unchanged (no card for a non-applicable edit)"
    );
    Ok(())
}

#[test]
fn test_pre_authorize_fs_tool_edit_preview_table() -> Result<()> {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        seed: Option<&'static str>,
        build_args: fn(&std::path::Path) -> serde_json::Value,
        expected_diff: Option<(&'static str, &'static str)>,
    }

    let cases = vec![
        Case {
            name: "create apply produces a diff preview",
            seed: None,
            build_args: |target| {
                json!({
                    "path": target.to_string_lossy(),
                    "operation": {"type": "create", "content": "hello\n"}
                })
            },
            expected_diff: Some(("+hello", "+hello")),
        },
        Case {
            name: "search_replace without expected_version produces a diff preview",
            seed: Some("hello\nworld\n"),
            build_args: |target| {
                json!({
                    "path": target.to_string_lossy(),
                    "mode": "apply",
                    "operation": {
                        "type": "search_replace",
                        "search": "world",
                        "replacement": "there"
                    }
                })
            },
            expected_diff: Some(("-world", "+there")),
        },
        Case {
            name: "stale expected_version produces no preview",
            seed: Some("hello\nworld\n"),
            build_args: |target| {
                json!({
                    "path": target.to_string_lossy(),
                    "mode": "apply",
                    "expected_version": "deadbeef",
                    "operation": {
                        "type": "search_replace",
                        "search": "world",
                        "replacement": "there"
                    }
                })
            },
            expected_diff: None,
        },
        Case {
            name: "noop produces no preview",
            seed: Some("hello\nworld\n"),
            build_args: |target| {
                json!({
                    "path": target.to_string_lossy(),
                    "mode": "apply",
                    "operation": {
                        "type": "search_replace",
                        "search": "world",
                        "replacement": "world"
                    }
                })
            },
            expected_diff: None,
        },
    ];

    // -- Exec & Check
    for case in cases {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("target.txt");
        if let Some(seed) = case.seed {
            std::fs::write(&target, seed)?;
        }
        let arguments = (case.build_args)(&target);
        let args = arguments
            .as_object()
            .ok_or("case arguments must be an object")?;

        let output = pre_authorize_fs_tool(Some(BuiltinKind::Edit), args, tmp.path());

        match case.expected_diff {
            Some((removed, added)) => {
                let output = output.ok_or("a valid edit plan must produce a card")?;
                let preview = output
                    .preview
                    .ok_or("a valid edit plan must produce the proposed diff")?;
                assert!(
                    preview.contains(removed) && preview.contains(added),
                    "case '{}': diff must contain {removed} and {added}, got: {preview:?}",
                    case.name
                );
                let ask_preview = output
                    .ask_context
                    .preview
                    .ok_or("edit apply must populate ask_context.preview")?;
                assert_eq!(
                    ask_preview, preview,
                    "case '{}': the ask context mirrors the preview",
                    case.name
                );
            }
            None => {
                assert!(
                    output.is_none(),
                    "case '{}': no diff to approve; pre-authorize must produce no card",
                    case.name
                );
            }
        }
    }
    Ok(())
}
