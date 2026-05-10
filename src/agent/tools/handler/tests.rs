use super::*;
use nu_protocol::{Span, Value};
use serde_json::json;
use std::cell::RefCell;
use std::fs;
use tempfile::tempdir;

fn empty_closure_registry() -> crate::tools::closure::ClosureRegistry {
    crate::tools::closure::ClosureRegistry::new()
}

#[test]
fn classify_source_treats_builtin_fs_read_as_closure_tool() {
    let closure_registry = empty_closure_registry();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());

    let source = super::classify_tool_source("read", &closure_registry, &mcp_registry);
    assert_eq!(source, Some(ToolSource::Closure));
}

#[test]
fn classify_source_treats_builtin_fs_edit_as_closure_tool() {
    let closure_registry = empty_closure_registry();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());

    let source = super::classify_tool_source("edit", &closure_registry, &mcp_registry);
    assert_eq!(source, Some(ToolSource::Closure));
}

#[test]
fn classify_source_treats_builtin_fs_patch_as_closure_tool() {
    let closure_registry = empty_closure_registry();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());

    let source = super::classify_tool_source("patch", &closure_registry, &mcp_registry);
    assert_eq!(source, Some(ToolSource::Closure));
}

#[test]
fn classify_validation_error_message_detects_missing_expected_version_contract() {
    assert!(super::classify_validation_error_message(
        "missing expected_version for mutating operation"
    ));
}

#[test]
fn builtin_fs_tool_name_detection_matches_exact_contract() {
    assert!(super::is_builtin_fs_tool_name("read"));
    assert!(super::is_builtin_fs_tool_name("edit"));
    assert!(super::is_builtin_fs_tool_name("patch"));
    assert!(super::is_builtin_fs_tool_name("skill"));

    assert!(!super::is_builtin_fs_tool_name("fs__read"));
    assert!(!super::is_builtin_fs_tool_name("tool__edit"));
}

#[test]
fn builtin_skill_dispatch_loads_from_explicit_resolver_for_cwd() {
    let dir = tempdir().expect("temp dir");
    let cwd = dir.path().join("repo");

    fs::create_dir_all(cwd.join(".agents/skills/context")).expect("local skill dir");
    fs::write(
        cwd.join(".agents/skills/context/SKILL.md"),
        "local context skill\n",
    )
    .expect("local skill file");
    fs::create_dir_all(&cwd).expect("cwd");

    let result = super::dispatch_builtin_fs_tool(
        "skill",
        &json!({
            "name": "context"
        }),
        cwd.as_path(),
    )
    .expect("dispatch should succeed")
    .expect("skill should be handled");

    assert_eq!(result["name"], "context");
    assert_eq!(result["source"], "local");
    assert_eq!(result["content"], "local context skill\n");
}

#[test]
fn builtin_skill_dispatch_preserves_missing_skill_not_found_semantics() {
    let dir = tempdir().expect("temp dir");
    let cwd = dir.path().join("repo");

    fs::create_dir_all(cwd.join(".agents/skills")).expect("local skills root");
    fs::create_dir_all(&cwd).expect("cwd");

    let result = super::dispatch_builtin_fs_tool(
        "skill",
        &json!({
            "name": "does-not-exist"
        }),
        cwd.as_path(),
    )
    .expect("dispatch should succeed")
    .expect("skill should be handled");

    assert_eq!(result["name"], "does-not-exist");
    assert_eq!(result["found"], false);
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
fn mcp_registry_gating_blocks_tool_when_server_disabled() {
    let registry = McpToolRegistry::from_tools(vec![crate::tools::mcp::client::McpToolDefinition {
        server: "gh".to_string(),
        name: "gh__list_prs".to_string(),
        raw_name: "list_prs".to_string(),
        description: None,
        parameters: None,
    }])
    .expect("registry");

    assert!(registry.contains("gh__list_prs"));
    registry
        .set_server_enabled("gh", false)
        .expect("disable server should succeed");
    assert!(!registry.contains("gh__list_prs"));
    assert!(registry.is_registered("gh__list_prs"));
}

#[test]
fn mcp_registry_reenable_restores_tool_visibility() {
    let registry = McpToolRegistry::from_tools(vec![crate::tools::mcp::client::McpToolDefinition {
        server: "gh".to_string(),
        name: "gh__list_prs".to_string(),
        raw_name: "list_prs".to_string(),
        description: None,
        parameters: None,
    }])
    .expect("registry");

    registry
        .set_server_enabled("gh", false)
        .expect("disable server");
    assert!(!registry.contains("gh__list_prs"));

    registry
        .set_server_enabled("gh", true)
        .expect("re-enable server");
    assert!(registry.contains("gh__list_prs"));
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

#[test]
fn builtin_read_dispatch_invokes_fs_read_file() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-read.txt");
    fs::write(&file, "a\nb\nc\n").expect("write");

    let result = super::dispatch_builtin_fs_tool(
        "read",
        &json!({
            "path": file.to_string_lossy(),
            "offset": 1,
            "limit": 1
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("read should be handled");

    assert_eq!(result["content"], "b\n");
    assert_eq!(result["total_lines"], 3);
    assert_eq!(result["offset"], 1);
    assert_eq!(result["limit"], 1);
}

#[test]
fn builtin_edit_dispatch_invokes_fs_apply_search_replace_edit() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "search": "beta",
            "replacement": "gamma",
            "expected_version": expected_version,
            "match_mode": "literal",
            "occurrence": "first"
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("edit should be handled");

    assert_eq!(result["changed"], true);
    assert_eq!(result["replacements"], 1);
    assert_eq!(fs::read_to_string(&file).expect("read"), "alpha gamma\n");
}

#[test]
fn builtin_edit_contract_preview_returns_envelope_and_does_not_write() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-preview.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "preview",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "beta",
                "replacement": "gamma",
                "match_mode": "literal",
                "occurrence": "first"
            }
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("edit should be handled");

    assert_eq!(result["proposal_id"], serde_json::Value::Null);
    assert_eq!(result["applied"], false);
    assert_eq!(result["would_change"], true);
    assert!(result["diff"].as_str().unwrap_or_default().contains("---"));
    assert!(result["stats"]["previous_bytes"].is_number());
    assert!(result["stats"]["new_bytes"].is_number());
    assert_eq!(result["stats"]["files_changed"], 1);
    assert_eq!(result["stats"]["insertions"], 1);
    assert_eq!(result["stats"]["deletions"], 1);
    assert_eq!(result["stats"]["diff_truncated"], false);
    assert_eq!(result["stats"]["omitted_files"], 0);
    assert_eq!(result["stats"]["omitted_hunks"], 0);
    assert!(result["diagnostics"].is_array());
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
}

#[test]
fn edit_preview_noop_replacement_reports_would_change_false() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-preview-noop.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "preview",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "beta",
                "replacement": "beta",
                "match_mode": "literal",
                "occurrence": "all"
            }
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("edit should be handled");

    assert_eq!(result["applied"], false);
    assert_eq!(result["would_change"], false);
    assert_eq!(result["changed"], false);
    assert_eq!(result["wrote"], false);
    assert_eq!(result["noop"], true);
    assert_eq!(result["diff"], "");
    assert_eq!(result["stats"]["replacements"], 1);
    assert_eq!(result["stats"]["files_changed"], 0);
    assert_eq!(result["stats"]["insertions"], 0);
    assert_eq!(result["stats"]["deletions"], 0);
    assert_eq!(result["stats"]["diff_truncated"], false);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
}

#[test]
fn builtin_edit_contract_preview_and_apply_share_planning_semantics() {
    let dir = tempdir().expect("temp dir");
    let preview_file = dir.path().join("dispatch-edit-preview-plan.txt");
    let apply_file = dir.path().join("dispatch-edit-apply-plan.txt");
    let content = "alpha beta alpha\n";
    fs::write(&preview_file, content).expect("write");
    fs::write(&apply_file, content).expect("write");
    let preview_expected_version = crate::tools::fs::core::version_token(content);
    let apply_expected_version = crate::tools::fs::core::version_token(content);

    let preview = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": preview_file.to_string_lossy(),
            "mode": "preview",
            "expected_version": preview_expected_version,
            "operation": {
                "type": "search_replace",
                "search": "alpha",
                "replacement": "omega",
                "match_mode": "literal",
                "occurrence": "all"
            }
        }),
        dir.path(),
    )
    .expect("preview dispatch")
    .expect("preview payload");

    let apply = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": apply_file.to_string_lossy(),
            "mode": "apply",
            "expected_version": apply_expected_version,
            "operation": {
                "type": "search_replace",
                "search": "alpha",
                "replacement": "omega",
                "match_mode": "literal",
                "occurrence": "all"
            }
        }),
        dir.path(),
    )
    .expect("apply dispatch")
    .expect("apply payload");

    assert_eq!(preview["would_change"], apply["would_change"]);
    assert_eq!(preview["diff"], apply["diff"]);
    assert_eq!(preview["stats"]["new_bytes"], apply["stats"]["new_bytes"]);
    assert_eq!(preview["applied"], false);
    assert_eq!(apply["applied"], true);
}

#[test]
fn builtin_edit_contract_legacy_payload_remains_compatible() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-legacy.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "search": "beta",
            "replacement": "gamma",
            "expected_version": expected_version,
            "match_mode": "literal",
            "occurrence": "first"
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("edit should be handled");

    assert_eq!(result["applied"], true);
    assert_eq!(result["would_change"], true);
    assert_eq!(result["changed"], true);
    assert_eq!(result["replacements"], 1);
    assert!(result.get("stats").is_some());
    assert!(result.get("diagnostics").is_some());
}

#[test]
fn builtin_edit_contract_stale_version_uses_stale_diagnostic_class() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-stale.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "apply",
            "expected_version": "stale-version",
            "search": "beta",
            "replacement": "gamma"
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("edit should be handled");

    assert_eq!(result["applied"], false);
    assert_eq!(result["diagnostics"][0]["class"], "stale");
}

fn mutate_edit_apply_file_for_conflict(path: &std::path::Path) {
    fs::write(path, "delta gamma\n").expect("mutate file for conflict");
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ApplyHookEvent {
    Preview,
    DecisionApprove,
    WriteAttempt,
}

thread_local! {
    static APPLY_HOOK_EVENTS: RefCell<Vec<ApplyHookEvent>> = const { RefCell::new(Vec::new()) };
}

fn reset_apply_hook_events() {
    APPLY_HOOK_EVENTS.with(|events| events.borrow_mut().clear());
}

fn record_preview_event(_path: &std::path::Path, _display: &crate::agent::protocol::event::ToolDisplay) {
    APPLY_HOOK_EVENTS.with(|events| events.borrow_mut().push(ApplyHookEvent::Preview));
}

fn record_decision_event(_path: &std::path::Path, decision: &super::EditWriteDecision) {
    match decision {
        super::EditWriteDecision::Approve => {
            APPLY_HOOK_EVENTS.with(|events| events.borrow_mut().push(ApplyHookEvent::DecisionApprove));
        }
        super::EditWriteDecision::Deny { .. } => {}
    }
}

fn record_write_attempt_event(_path: &std::path::Path) {
    APPLY_HOOK_EVENTS.with(|events| events.borrow_mut().push(ApplyHookEvent::WriteAttempt));
}

fn snapshot_apply_hook_events() -> Vec<ApplyHookEvent> {
    APPLY_HOOK_EVENTS.with(|events| events.borrow().clone())
}

#[test]
fn edit_apply_conflict_response_is_coherent_with_actual_outcome() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-apply-conflict.txt");
    let original = "alpha beta\n";
    let mutated = "delta gamma\n";
    fs::write(&file, original).expect("seed file");
    let expected_version = crate::tools::fs::core::version_token(original);

    super::set_edit_apply_post_plan_hook(Some(mutate_edit_apply_file_for_conflict));

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "apply",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "beta",
                "replacement": "omega",
                "match_mode": "literal",
                "occurrence": "first"
            }
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("edit should be handled");

    super::set_edit_apply_post_plan_hook(None);

    assert_eq!(result["applied"], false);
    assert_eq!(result["conflict"], true);
    assert_eq!(result["would_change"], false);
    assert_eq!(result["changed"], false);
    assert_eq!(result["wrote"], false);
    assert_eq!(result["noop"], false);
    assert_eq!(result["stats"]["replacements"], 0);
    assert_eq!(result["diff"], "");
    assert_eq!(result["diagnostics"][0]["class"], "stale");
    assert_eq!(fs::read_to_string(&file).expect("read"), mutated);
}

#[test]
fn edit_apply_emits_preview_display_before_write_attempt() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-preview-before-write.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    reset_apply_hook_events();
    super::set_edit_apply_preview_hook(Some(record_preview_event));
    super::set_edit_apply_post_plan_hook(Some(record_write_attempt_event));

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "apply",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "beta",
                "replacement": "gamma",
                "match_mode": "literal",
                "occurrence": "first"
            }
        }),
        dir.path(),
    )
    .expect("dispatch")
    .expect("payload");

    super::set_edit_apply_preview_hook(None);
    super::set_edit_apply_post_plan_hook(None);

    let events = snapshot_apply_hook_events();
    assert_eq!(
        events,
        vec![ApplyHookEvent::Preview, ApplyHookEvent::WriteAttempt]
    );
    assert!(result.get("display").is_some(), "apply result must include direct preview display payload");
    assert_eq!(result["display"]["sections"][0]["language"], "diff");
}

#[test]
fn edit_apply_uses_autoapprove_decision_hook_before_write() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-autoapprove-decision.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    reset_apply_hook_events();
    super::set_edit_apply_decision_hook(Some(record_decision_event));
    super::set_edit_apply_post_plan_hook(Some(record_write_attempt_event));

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "apply",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "beta",
                "replacement": "gamma",
                "match_mode": "literal",
                "occurrence": "first"
            }
        }),
        dir.path(),
    )
    .expect("dispatch")
    .expect("payload");

    super::set_edit_apply_decision_hook(None);
    super::set_edit_apply_post_plan_hook(None);

    let events = snapshot_apply_hook_events();
    assert_eq!(
        events,
        vec![ApplyHookEvent::DecisionApprove, ApplyHookEvent::WriteAttempt]
    );
    assert_eq!(result["applied"], true);
    assert_eq!(result["wrote"], true);
}

#[test]
fn edit_apply_conflict_semantics_remain_unchanged_with_implicit_preview() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-apply-conflict-implicit-preview.txt");
    let original = "alpha beta\n";
    let mutated = "delta gamma\n";
    fs::write(&file, original).expect("seed file");
    let expected_version = crate::tools::fs::core::version_token(original);

    super::set_edit_apply_post_plan_hook(Some(mutate_edit_apply_file_for_conflict));

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "apply",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "beta",
                "replacement": "omega",
                "match_mode": "literal",
                "occurrence": "first"
            }
        }),
        dir.path(),
    )
    .expect("dispatch")
    .expect("payload");

    super::set_edit_apply_post_plan_hook(None);

    assert_eq!(result["applied"], false);
    assert_eq!(result["conflict"], true);
    assert_eq!(result["would_change"], false);
    assert_eq!(result["changed"], false);
    assert_eq!(result["wrote"], false);
    assert_eq!(result["noop"], false);
    assert_eq!(result["stats"]["replacements"], 0);
    assert_eq!(result["diff"], "");
    assert_eq!(result["diagnostics"][0]["class"], "stale");
    assert!(result.get("display").is_some());
    assert_eq!(fs::read_to_string(&file).expect("read"), mutated);
}

#[test]
fn edit_preview_mode_remains_supported_as_read_only() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-preview-read-only.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("seed file");
    let expected_version = crate::tools::fs::core::version_token(content);

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "preview",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "beta",
                "replacement": "gamma",
                "match_mode": "literal",
                "occurrence": "first"
            }
        }),
        dir.path(),
    )
    .expect("dispatch")
    .expect("payload");

    assert_eq!(result["mode"], "preview");
    assert_eq!(result["applied"], false);
    assert_eq!(result["would_change"], true);
    assert_eq!(result["wrote"], false);
    assert_eq!(fs::read_to_string(&file).expect("read"), content);
}

#[test]
fn builtin_edit_contract_invalid_mode_uses_validation_diagnostic_class() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-invalid-mode.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "invalid",
            "expected_version": expected_version,
            "search": "beta",
            "replacement": "gamma"
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("edit should be handled");

    assert_eq!(result["applied"], false);
    assert_eq!(result["diagnostics"][0]["class"], "validation");
}

#[test]
fn edit_contract_error_taxonomy_maps_permission_conflict_internal_classes_deterministically() {
    let permission = super::BuiltinFsToolError {
        kind: ToolErrorKind::Runtime,
        message: "opaque runtime error".to_string(),
        details: Some(json!({ "diagnostic_class": "permission" })),
    };
    assert_eq!(super::map_edit_contract_error(&permission), "permission");

    let conflict = super::BuiltinFsToolError {
        kind: ToolErrorKind::Validation,
        message: "version conflict: expected 'x', current 'y'".to_string(),
        details: Some(json!({ "diagnostic_class": "stale" })),
    };
    assert_eq!(super::map_edit_contract_error(&conflict), "stale");

    let internal = super::BuiltinFsToolError {
        kind: ToolErrorKind::Runtime,
        message: "i/o failure".to_string(),
        details: None,
    };
    assert_eq!(super::map_edit_contract_error(&internal), "internal");
}

#[test]
fn edit_diagnostics_taxonomy_is_deterministic_without_substring_matching() {
    let runtime_with_permission_word = super::BuiltinFsToolError {
        kind: ToolErrorKind::Runtime,
        message: "this mentions permission but no typed mapping".to_string(),
        details: None,
    };
    assert_eq!(
        super::map_edit_contract_error(&runtime_with_permission_word),
        "internal"
    );

    let runtime_with_typed_permission = super::BuiltinFsToolError {
        kind: ToolErrorKind::Runtime,
        message: "opaque runtime error".to_string(),
        details: Some(json!({ "diagnostic_class": "permission" })),
    };
    assert_eq!(
        super::map_edit_contract_error(&runtime_with_typed_permission),
        "permission"
    );

    let runtime_with_unknown_typed_class = super::BuiltinFsToolError {
        kind: ToolErrorKind::Runtime,
        message: "opaque runtime error".to_string(),
        details: Some(json!({ "diagnostic_class": "something-else" })),
    };
    assert_eq!(
        super::map_edit_contract_error(&runtime_with_unknown_typed_class),
        "internal"
    );
}

#[test]
fn builtin_patch_dispatch_invokes_fs_apply_line_range_patch_batch() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-patch.txt");
    let content = "a\nb\nc\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    let result = super::dispatch_builtin_fs_tool(
        "patch",
        &json!({
            "path": file.to_string_lossy(),
            "expected_version": expected_version,
            "operations": [
                {
                    "range": { "start": 2, "end": 2 },
                    "replacement": "beta\n"
                }
            ]
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("patch should be handled");

    assert_eq!(result["changed"], true);
    assert_eq!(result["operation_count"], 1);
    assert_eq!(fs::read_to_string(&file).expect("read"), "a\nbeta\nc\n");
}

#[test]
fn builtin_edit_dispatch_missing_expected_version_returns_validation_failure() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("dispatch-edit-missing-version.txt");
    fs::write(&file, "alpha beta\n").expect("write");

    let result = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "search": "beta",
            "replacement": "gamma"
        }),
        dir.path(),
    )
    .expect("dispatch should succeed")
    .expect("edit should be handled");

    assert_eq!(result["applied"], false);
    assert_eq!(result["would_change"], false);
    assert_eq!(result["diagnostics"][0]["class"], "validation");
    assert!(result["diagnostics"][0]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("missing expected_version"));
}

#[test]
fn builtin_fs_path_resolution_joins_relative_path_with_cwd() {
    let dir = tempdir().expect("temp dir");
    let cwd = dir.path();

    let relative_name = "sample.txt";
    let resolved = super::resolve_builtin_fs_path_for_cwd(relative_name, cwd);
    assert_eq!(resolved, cwd.join(relative_name));

    let absolute_input = cwd.join("already-absolute.txt");
    let absolute = super::resolve_builtin_fs_path_for_cwd(
        absolute_input.to_string_lossy().as_ref(),
        cwd,
    );
    assert_eq!(absolute, absolute_input);
}

fn tool_definition_named(name: &str) -> rig::completion::ToolDefinition {
    rig::completion::ToolDefinition {
        name: name.to_string(),
        description: format!("tool {name}"),
        parameters: json!({"type":"object"}),
    }
}

#[test]
fn canonical_llm_tool_definition_path_hides_disabled_mcp_tools() {
    let registry = McpToolRegistry::from_tools(vec![crate::tools::mcp::client::McpToolDefinition {
        server: "gh".to_string(),
        name: "gh__list_prs".to_string(),
        raw_name: "list_prs".to_string(),
        description: None,
        parameters: None,
    }])
    .expect("registry");

    let all_tools = vec![
        tool_definition_named("read"),
        tool_definition_named("gh__list_prs"),
    ];

    let initially_visible = super::llm_visible_tool_definitions(&all_tools, &registry);
    assert!(initially_visible.iter().any(|tool| tool.name == "gh__list_prs"));

    registry
        .set_server_enabled("gh", false)
        .expect("disable server should succeed");

    let after_disable = super::llm_visible_tool_definitions(&all_tools, &registry);
    assert!(after_disable.iter().all(|tool| tool.name != "gh__list_prs"));
    assert!(after_disable.iter().any(|tool| tool.name == "read"));
}

#[test]
fn canonical_llm_tool_definition_path_reveals_mcp_tools_after_reenable() {
    let registry = McpToolRegistry::from_tools(vec![crate::tools::mcp::client::McpToolDefinition {
        server: "gh".to_string(),
        name: "gh__list_prs".to_string(),
        raw_name: "list_prs".to_string(),
        description: None,
        parameters: None,
    }])
    .expect("registry");

    let all_tools = vec![tool_definition_named("gh__list_prs")];

    registry
        .set_server_enabled("gh", false)
        .expect("disable server should succeed");
    assert!(
        super::llm_visible_tool_definitions(&all_tools, &registry)
            .iter()
            .all(|tool| tool.name != "gh__list_prs")
    );

    registry
        .set_server_enabled("gh", true)
        .expect("re-enable server should succeed");

    let after_enable = super::llm_visible_tool_definitions(&all_tools, &registry);
    assert!(after_enable.iter().any(|tool| tool.name == "gh__list_prs"));
}

#[test]
fn canonical_llm_tool_definition_path_has_no_stale_mcp_exposure_after_disable() {
    let registry = McpToolRegistry::from_tools(vec![crate::tools::mcp::client::McpToolDefinition {
        server: "gh".to_string(),
        name: "gh__list_prs".to_string(),
        raw_name: "list_prs".to_string(),
        description: None,
        parameters: None,
    }])
    .expect("registry");

    let all_tools = vec![tool_definition_named("gh__list_prs")];

    let before_disable = super::llm_visible_tool_definitions(&all_tools, &registry);
    assert!(before_disable.iter().any(|tool| tool.name == "gh__list_prs"));

    registry
        .set_server_enabled("gh", false)
        .expect("disable server should succeed");

    let next_turn_visible = super::llm_visible_tool_definitions(&all_tools, &registry);
    assert!(
        next_turn_visible
            .iter()
            .all(|tool| tool.name != "gh__list_prs"),
        "next canonical tool-definition snapshot must not expose disabled MCP tools"
    );
}

#[test]
fn direct_tool_display_contract_accepts_minimal_sections_shape() {
    let payload = json!({
        "display": {
            "title": "edit sample.txt",
            "sections": [
                {
                    "label": "sample.txt",
                    "language": "diff",
                    "content": "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n"
                }
            ]
        },
        "ok": true
    });

    let display = super::build_direct_tool_display("patch", &payload).expect("display expected");
    assert_eq!(display.title, "edit sample.txt");
    assert_eq!(display.sections.len(), 1);
    assert_eq!(display.sections[0].label, "sample.txt");
    assert_eq!(display.sections[0].language, "diff");
    assert!(display.sections[0].stats.is_none());
}

#[test]
fn edit_preview_emits_single_display_section() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("single-section.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    let payload = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "preview",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "beta",
                "replacement": "gamma",
                "match_mode": "literal",
                "occurrence": "first"
            }
        }),
        dir.path(),
    )
    .expect("dispatch")
    .expect("payload");

    let display = super::build_direct_tool_display("edit", &payload).expect("display expected");
    assert_eq!(display.sections.len(), 1);
    assert_eq!(display.sections[0].label, file.to_string_lossy().to_string());
    assert_eq!(display.sections[0].language, "diff");
}

#[test]
fn tool_display_preserves_machine_payload_contract() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("machine-contract.txt");
    let content = "alpha beta\n";
    fs::write(&file, content).expect("write");
    let expected_version = crate::tools::fs::core::version_token(content);

    let payload = super::dispatch_builtin_fs_tool(
        "edit",
        &json!({
            "path": file.to_string_lossy(),
            "mode": "preview",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "beta",
                "replacement": "gamma",
                "match_mode": "literal",
                "occurrence": "first"
            }
        }),
        dir.path(),
    )
    .expect("dispatch")
    .expect("payload");

    let machine_diff = payload["diff"].as_str().unwrap_or_default().to_string();
    let machine_files_changed = payload["stats"]["files_changed"].as_u64();
    let display = super::build_direct_tool_display("edit", &payload).expect("display expected");

    assert_eq!(payload["mode"], "preview");
    assert_eq!(payload["would_change"], true);
    assert_eq!(display.sections[0].content, machine_diff);
    assert_eq!(
        display.sections[0].stats.as_ref().and_then(|stats| stats.files_changed),
        machine_files_changed.map(|v| v as usize)
    );
}
