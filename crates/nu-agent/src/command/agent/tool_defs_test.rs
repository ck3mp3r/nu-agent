use super::tool_defs::{
    assemble_tool_definitions, builtin_tool_definitions, messaging_tool_definitions,
    orchestrator_messaging_tool_definitions, orchestrator_tool_definitions,
};

#[test]
fn builtin_tool_registration_contains_exact_unprefixed_names() {
    let names = builtin_tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec!["read", "edit", "patch", "skill", "http", "grep", "glob"]
    );
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
    use nu_agent_core::protocol::persona::PersonaSummary;
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
fn messaging_tool_registration_contains_only_send_message() {
    let names = messaging_tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["send_message"]);
}

#[test]
fn orchestrator_messaging_tool_registration_contains_only_list_agents() {
    let names = orchestrator_messaging_tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["list_agents"]);
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
        send.description.contains("asynchronously"),
        "Expected async mention, got: {}",
        send.description
    );
    assert!(
        send.description.contains("task instructions"),
        "Expected task instructions mention, got: {}",
        send.description
    );
    assert!(
        !send.description.contains("list_agents"),
        "send_message description must not reference list_agents (sub-agents cannot use it), got: {}",
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
    assert_eq!(required, vec!["path", "operation"]);

    assert_eq!(edit.parameters["properties"]["mode"]["enum"][0], "preview");
    assert_eq!(edit.parameters["properties"]["mode"]["enum"][1], "apply");

    let op_types = &edit.parameters["properties"]["operation"]["properties"]["type"]["enum"];
    assert_eq!(op_types[0], "search_replace");
    assert_eq!(op_types[1], "create");
}

// --- Tool assembly tests: all agents always have all tools ---

#[test]
fn all_agents_have_spawn_agent_send_message_and_list_agents() {
    // All tool groups are always registered unconditionally.
    // The permission system (allow/ask/deny) gates actual use at call time.
    let closure_registry = nu_agent_core::tools::closure::ClosureRegistry::default();
    let agents_config = nu_agent_core::config::AgentsConfig::default();
    let cwd = std::path::Path::new("/tmp");

    let assembly = assemble_tool_definitions(&closure_registry, &agents_config, &[], cwd);

    let names: Vec<&str> = assembly
        .tool_definitions
        .iter()
        .map(|t| t.name.as_str())
        .collect();

    assert!(
        names.contains(&"send_message"),
        "All agents must have send_message, got: {names:?}"
    );
    assert!(
        names.contains(&"list_agents"),
        "All agents must have list_agents, got: {names:?}"
    );
    assert!(
        names.contains(&"spawn_agent"),
        "All agents must have spawn_agent, got: {names:?}"
    );
    assert!(
        names.contains(&"terminate_agent"),
        "All agents must have terminate_agent, got: {names:?}"
    );
}
