use super::*;

#[test]
fn display_tool_name_renders_args_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, serde_json::Value, &str)> = vec![
        ("no args", "skill", serde_json::json!({}), "skill"),
        (
            "single arg",
            "skill",
            serde_json::json!({"name": "nushell-shell"}),
            "skill(name=nushell-shell)",
        ),
        (
            "multiple args sorted alphabetically",
            "edit",
            serde_json::json!({
                "filePath": "src/foo.rs",
                "oldString": "foo",
                "newString": "bar"
            }),
            "edit(filePath=src/foo.rs, newString=bar, oldString=foo)",
        ),
        (
            "skips null values",
            "edit",
            serde_json::json!({
                "filePath": "test.rs",
                "mode": null,
                "content": "hello"
            }),
            "edit(content=hello, filePath=test.rs)",
        ),
        (
            "all null args",
            "test",
            serde_json::json!({"arg1": null, "arg2": null}),
            "test",
        ),
        (
            "array as compact json",
            "list",
            serde_json::json!({"items": [1, 2, 3]}),
            "list(items=[1,2,3])",
        ),
        (
            "boolean values",
            "flag",
            serde_json::json!({"enabled": true, "verbose": false}),
            "flag(enabled=true, verbose=false)",
        ),
        (
            "number values",
            "config",
            serde_json::json!({"count": 42, "ratio": 2.72}),
            "config(count=42, ratio=2.72)",
        ),
    ];

    // -- Exec & Check
    for (name, tool_name, args, expected) in cases {
        assert_eq!(
            display_tool_name(tool_name, &args),
            expected,
            "case '{name}' must render the expected display name"
        );
    }
}

#[test]
fn display_tool_name_non_object_args_returns_tool_name_only() {
    for args in [
        serde_json::json!("string"),
        serde_json::json!(123),
        serde_json::json!(true),
        serde_json::json!(null),
        serde_json::json!([]),
    ] {
        assert_eq!(display_tool_name("test", &args), "test");
    }
}

#[test]
fn display_tool_name_truncates_long_values() {
    // -- Setup & Fixtures
    let long_value = "a".repeat(70);
    let long_obj = serde_json::json!({
        "key1": "value1",
        "key2": "value2",
        "key3": "value3",
        "key4": "value4",
        "key5": "value5",
        "key6": "value6"
    });
    let long_array: Vec<i32> = (0..100).collect();

    // -- Exec & Check
    let truncated_string = display_tool_name("read", &serde_json::json!({"filePath": long_value}));
    assert_eq!(
        truncated_string,
        format!("read(filePath={}…)", "a".repeat(60))
    );

    let truncated_object = display_tool_name("tool", &serde_json::json!({"data": long_obj}));
    assert!(truncated_object.starts_with("tool(data="));
    assert!(truncated_object.ends_with("…)"));
    assert!(truncated_object.contains("…"));

    let truncated_array = display_tool_name("batch", &serde_json::json!({"values": long_array}));
    assert!(truncated_array.starts_with("batch(values="));
    assert!(truncated_array.ends_with("…)"));
    assert!(truncated_array.contains("…"));
}

#[test]
fn display_tool_name_shows_nested_object_as_compact_json() {
    let result = display_tool_name(
        "complex",
        &serde_json::json!({
            "config": {"key": "value", "nested": {"deep": true}}
        }),
    );
    assert!(result.starts_with("complex(config="));
    assert!(result.contains(r#"{"key":"value""#));
}
