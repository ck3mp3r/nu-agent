use super::tool_defs::{
    builtin_tool_definitions, messaging_tool_definitions, orchestrator_tool_definitions,
};

#[test]
fn builtin_tool_registration_contains_exact_unprefixed_names() {
    let names = builtin_tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["read", "edit", "patch", "skill"]);
}

#[test]
fn orchestrator_tool_registration_contains_exact_names() {
    let names = orchestrator_tool_definitions(&[])
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["spawn_agent", "terminate_agent"]);
}

#[test]
fn orchestrator_tool_description_shows_no_agents_message_when_empty() {
    let defs = orchestrator_tool_definitions(&[]);
    let desc = &defs[0].description;
    assert!(
        desc.contains("No agent personas found"),
        "Expected no-agents message, got: {desc}"
    );
}

#[test]
fn orchestrator_tool_description_lists_available_agents() {
    use crate::agent::protocol::persona::PersonaSummary;
    let agents = vec![
        PersonaSummary {
            name: "coder".to_string(),
            description: Some("Writes code".to_string()),
            builtin: false,
        },
        PersonaSummary {
            name: "reviewer".to_string(),
            description: None,
            builtin: false,
        },
    ];
    let defs = orchestrator_tool_definitions(&agents);
    let desc = &defs[0].description;
    assert!(
        desc.contains("coder: Writes code"),
        "Expected coder entry, got: {desc}"
    );
    assert!(
        desc.contains("- reviewer"),
        "Expected reviewer entry, got: {desc}"
    );
    assert!(
        desc.contains("send_message"),
        "Expected send_message mention, got: {desc}"
    );
    assert!(desc.contains("tmux"), "Expected tmux mention, got: {desc}");
}

#[test]
fn messaging_tool_registration_contains_exact_names() {
    let names = messaging_tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["send_message", "list_agents"]);
}

#[test]
fn send_message_description_explains_delivery_semantics() {
    let defs = messaging_tool_definitions();
    let send = defs
        .iter()
        .find(|d| d.name == "send_message")
        .expect("send_message tool");
    assert!(
        send.description.contains("conversation turns"),
        "Expected delivery semantics, got: {}",
        send.description
    );
    assert!(
        send.description.contains("list_agents"),
        "Expected list_agents mention, got: {}",
        send.description
    );
    assert!(
        send.description.contains("asynchronously"),
        "Expected async mention, got: {}",
        send.description
    );
}

#[test]
fn builtin_tool_registration_explicitly_rejects_prefixed_names() {
    let names = builtin_tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    assert!(!names.iter().any(|name| name.starts_with("fs__")));
    assert!(!names.iter().any(|name| name.starts_with("tool__")));
}

#[test]
fn builtin_edit_definition_uses_mode_and_operation_contract_with_legacy_compat_fields() {
    let edit = builtin_tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "edit")
        .expect("edit tool definition");

    let required = edit.parameters["required"]
        .as_array()
        .expect("required array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    assert_eq!(required, vec!["path", "expected_version"]);

    assert_eq!(edit.parameters["properties"]["mode"]["enum"][0], "preview");
    assert_eq!(edit.parameters["properties"]["mode"]["enum"][1], "apply");
    assert_eq!(
        edit.parameters["properties"]["operation"]["required"],
        serde_json::json!(["search", "replacement"])
    );

    assert!(
        edit.parameters["properties"]["search"]["description"]
            .as_str()
            .unwrap_or_default()
            .contains("legacy")
    );
    assert!(
        edit.parameters["properties"]["replacement"]["description"]
            .as_str()
            .unwrap_or_default()
            .contains("legacy")
    );
}
