use super::*;
use nu_protocol::{Span, Value};
use serde_json::json;
fn empty_closure_registry() -> crate::tools::closure::ClosureRegistry {
    crate::tools::closure::ClosureRegistry::new()
}

#[test]
fn json_to_nu_value_converts_string() {
    let json = json!("hello");
    let span = Span::test_data();
    let result = json_to_nu_value(&json, span).unwrap();

    assert_eq!(result.as_str().unwrap(), "hello");
}

#[test]
fn json_to_nu_value_converts_number() {
    let json = json!(42);
    let span = Span::test_data();
    let result = json_to_nu_value(&json, span).unwrap();

    assert_eq!(result.as_int().unwrap(), 42);
}

#[test]
fn json_to_nu_value_converts_float() {
    let json = json!(2.5);
    let span = Span::test_data();
    let result = json_to_nu_value(&json, span).unwrap();

    assert_eq!(result.as_float().unwrap(), 2.5);
}

#[test]
fn json_to_nu_value_converts_bool() {
    let json_true = json!(true);
    let json_false = json!(false);
    let span = Span::test_data();

    let result_true = json_to_nu_value(&json_true, span).unwrap();
    let result_false = json_to_nu_value(&json_false, span).unwrap();

    assert!(result_true.as_bool().unwrap());
    assert!(!result_false.as_bool().unwrap());
}

#[test]
fn json_to_nu_value_converts_null() {
    let json = json!(null);
    let span = Span::test_data();
    let result = json_to_nu_value(&json, span).unwrap();

    assert!(result.is_nothing());
}

#[test]
fn json_to_nu_value_converts_array() {
    let json = json!([1, 2, 3]);
    let span = Span::test_data();
    let result = json_to_nu_value(&json, span).unwrap();

    let list = result.as_list().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].as_int().unwrap(), 1);
    assert_eq!(list[1].as_int().unwrap(), 2);
    assert_eq!(list[2].as_int().unwrap(), 3);
}

#[test]
fn json_to_nu_value_converts_object() {
    let json = json!({"name": "test", "value": 42});
    let span = Span::test_data();
    let result = json_to_nu_value(&json, span).unwrap();

    let record = result.as_record().unwrap();
    assert_eq!(record.get("name").unwrap().as_str().unwrap(), "test");
    assert_eq!(record.get("value").unwrap().as_int().unwrap(), 42);
}

#[test]
fn nu_value_to_json_converts_string() {
    let value = Value::string("hello", Span::test_data());
    let result = nu_value_to_json(&value).unwrap();

    assert_eq!(result, json!("hello"));
}

#[test]
fn nu_value_to_json_converts_int() {
    let value = Value::int(42, Span::test_data());
    let result = nu_value_to_json(&value).unwrap();

    assert_eq!(result, json!(42));
}

#[test]
fn nu_value_to_json_converts_float() {
    let value = Value::float(2.5, Span::test_data());
    let result = nu_value_to_json(&value).unwrap();

    assert_eq!(result, json!(2.5));
}

#[test]
fn nu_value_to_json_converts_bool() {
    let value_true = Value::bool(true, Span::test_data());
    let value_false = Value::bool(false, Span::test_data());

    let result_true = nu_value_to_json(&value_true).unwrap();
    let result_false = nu_value_to_json(&value_false).unwrap();

    assert_eq!(result_true, json!(true));
    assert_eq!(result_false, json!(false));
}

#[test]
fn nu_value_to_json_converts_nothing() {
    let value = Value::nothing(Span::test_data());
    let result = nu_value_to_json(&value).unwrap();

    assert_eq!(result, json!(null));
}

#[test]
fn nu_value_to_json_converts_list() {
    let value = Value::list(
        vec![
            Value::int(1, Span::test_data()),
            Value::int(2, Span::test_data()),
            Value::int(3, Span::test_data()),
        ],
        Span::test_data(),
    );
    let result = nu_value_to_json(&value).unwrap();

    assert_eq!(result, json!([1, 2, 3]));
}

#[test]
fn nu_value_to_json_converts_record() {
    let mut record = nu_protocol::record!();
    record.insert("name".to_string(), Value::string("test", Span::test_data()));
    record.insert("value".to_string(), Value::int(42, Span::test_data()));

    let value = Value::record(record, Span::test_data());
    let result = nu_value_to_json(&value).unwrap();

    assert_eq!(result, json!({"name": "test", "value": 42}));
}

#[test]
fn nu_value_to_json_handles_nested_structures() {
    let inner_record = Value::record(
        nu_protocol::record!(
            "x" => Value::int(1, Span::test_data()),
            "y" => Value::int(2, Span::test_data())
        ),
        Span::test_data(),
    );

    let value = Value::list(vec![inner_record], Span::test_data());
    let result = nu_value_to_json(&value).unwrap();

    assert_eq!(result, json!([{"x": 1, "y": 2}]));
}

#[test]
fn classify_source_identifies_mcp_membership() {
    let closure_registry = empty_closure_registry();
    let mcp_registry = McpToolRegistry::from_names(["k8s__list_pods"]);

    let source = super::classify_tool_source("k8s__list_pods", &closure_registry, &mcp_registry);
    assert_eq!(source, Some(ToolSource::Mcp));
}

#[test]
fn classify_source_returns_none_for_unknown_tool() {
    let closure_registry = empty_closure_registry();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());

    let source = super::classify_tool_source("unknown/tool", &closure_registry, &mcp_registry);
    assert!(source.is_none());
}

#[test]
fn classify_source_requires_namespaced_mcp_tool_name() {
    let closure_registry = empty_closure_registry();
    let mcp_registry = McpToolRegistry::from_names(["gh__list_prs"]);

    let namespaced = super::classify_tool_source("gh__list_prs", &closure_registry, &mcp_registry);
    let raw = super::classify_tool_source("list_prs", &closure_registry, &mcp_registry);

    assert_eq!(namespaced, Some(ToolSource::Mcp));
    assert!(raw.is_none());
}

#[test]
fn unknown_tool_error_mentions_exposed_namespaced_name() {
    let name = "gh__list_prs";
    let err = nu_protocol::shell_error::generic::GenericError::new(
        format!("Tool '{}' not found", name),
        "Unknown tool",
        Span::test_data(),
    );

    assert!(err.error.contains(name));
}

#[test]
fn mcp_registry_resolves_raw_name_from_exposed_name() {
    let registry =
        McpToolRegistry::from_tools(vec![crate::tools::mcp::client::McpToolDefinition {
            server: "gh".to_string(),
        name: "gh__list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        }])
        .expect("registry should build");

    assert_eq!(registry.raw_name_for("gh__list_prs"), Some("list_prs"));
    assert_eq!(registry.raw_name_for("list_prs"), None);
}

#[test]
fn mcp_registry_rejects_duplicate_exposed_names() {
    let result = McpToolRegistry::from_tools(vec![
        crate::tools::mcp::client::McpToolDefinition {
            server: "gh".to_string(),
            name: "gh__list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        },
        crate::tools::mcp::client::McpToolDefinition {
            server: "gh".to_string(),
            name: "gh__list_prs".to_string(),
            raw_name: "list_pull_requests".to_string(),
            description: None,
            parameters: None,
        },
    ]);

    assert!(result.is_err());
    assert!(
        result
            .expect_err("must error on duplicate exposed names")
            .contains("duplicate exposed MCP tool name")
    );
}

#[test]
fn resolve_mcp_invocation_name_uses_raw_name_mapping() {
    let registry =
        McpToolRegistry::from_tools(vec![crate::tools::mcp::client::McpToolDefinition {
            server: "gh".to_string(),
        name: "gh__list_prs".to_string(),
            raw_name: "list_prs".to_string(),
            description: None,
            parameters: None,
        }])
        .expect("registry should build");

    assert_eq!(
        super::resolve_mcp_invocation_name(&registry, "gh__list_prs"),
        Some("list_prs")
    );
    assert_eq!(
        super::resolve_mcp_invocation_name(&registry, "gh__missing"),
        None
    );
}

#[test]
fn unknown_tool_builds_non_fatal_failure_result() {
    let tool_call = rig::completion::message::ToolCall::new(
        "call_unknown".to_string(),
        rig::completion::message::ToolFunction::new("missing::tool".to_string(), json!({})),
    );

    let result = super::build_failure_result(
        &tool_call,
        ToolSource::Unknown,
        ToolErrorKind::Unknown,
        "Tool 'missing::tool' not found".to_string(),
        None,
    );

    let failure = result
        .failure
        .as_ref()
        .expect("unknown tool should produce failure payload");
    assert_eq!(failure.source, ToolSource::Unknown);
    assert_eq!(failure.error_kind, ToolErrorKind::Unknown);

    let content: serde_json::Value = serde_json::from_str(&result.content).expect("json payload");
    assert_eq!(content["tool_name"], "missing::tool");
    assert_eq!(content["tool_call_id"], "call_unknown");
    assert_eq!(content["source"], "unknown");
    assert_eq!(content["error_kind"], "unknown");
}

#[test]
fn failure_payload_contract_contains_required_fields() {
    let failure = ToolFailureOutcome {
        tool_name: "gh__list_prs".to_string(),
        tool_call_id: "call_1".to_string(),
        source: ToolSource::Mcp,
        error_kind: ToolErrorKind::Transport,
        message: "connection reset".to_string(),
        details: Some(json!({"retryable": true})),
    };

    let payload = failure.to_json_value();
    assert_eq!(payload["tool_name"], "gh__list_prs");
    assert_eq!(payload["tool_call_id"], "call_1");
    assert_eq!(payload["source"], "mcp");
    assert_eq!(payload["error_kind"], "transport");
    assert_eq!(payload["message"], "connection reset");
    assert_eq!(payload["details"]["retryable"], true);
}
