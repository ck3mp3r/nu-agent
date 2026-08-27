use super::{
    build_compaction_params, builtin_tool_definitions, merge_compaction_configs,
    messaging_tool_definitions, orchestrator_tool_definitions,
};
use crate::compaction::{CompactionParams, CompactionStrategy};
use crate::config::CompactionConfig;
use crate::protocol::persona::PersonaSummary;

// ── merge_compaction_configs ─────────────────────────────────────────────────

#[test]
fn merge_cli_overrides_plugin_config() {
    let plugin = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        proactive_threshold_pct: Some(0.85),
    };

    let cli = CompactionConfig {
        strategy: None,
        proactive_threshold_pct: None,
    };

    let merged = merge_compaction_configs(Some(&plugin), &cli);

    assert_eq!(merged.strategy, Some(CompactionStrategy::SlidingSummary));
    assert_eq!(merged.proactive_threshold_pct, Some(0.85));
}

#[test]
fn merge_plugin_config_overrides_default() {
    let plugin = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        proactive_threshold_pct: Some(0.9),
    };

    let cli = CompactionConfig::default();

    let merged = merge_compaction_configs(Some(&plugin), &cli);

    assert_eq!(merged.strategy, Some(CompactionStrategy::SlidingSummary));
    assert_eq!(merged.proactive_threshold_pct, Some(0.9));
}

#[test]
fn merge_default_used_when_no_config() {
    let cli = CompactionConfig::default();
    let merged = merge_compaction_configs(None, &cli);

    assert!(merged.strategy.is_none());
    assert!(merged.proactive_threshold_pct.is_none());
}

// ── build_compaction_params ──────────────────────────────────────────────────

#[test]
fn build_compaction_params_applies_merged_values() {
    let merged = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        proactive_threshold_pct: Some(0.9),
    };

    let config = build_compaction_params(&merged);

    assert_eq!(
        config.compaction_strategy,
        CompactionStrategy::SlidingSummary
    );
}

#[test]
fn build_compaction_params_uses_defaults_when_none() {
    let merged = CompactionConfig::default();
    let config = build_compaction_params(&merged);
    let defaults = CompactionParams::default();

    assert_eq!(config.compaction_strategy, defaults.compaction_strategy);
}

#[test]
fn build_compaction_params_partial_override() {
    let merged = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        proactive_threshold_pct: None,
    };

    let config = build_compaction_params(&merged);
    let defaults = CompactionParams::default();

    assert_eq!(
        config.compaction_strategy,
        CompactionStrategy::SlidingSummary
    );
    assert_eq!(config.compaction_strategy, defaults.compaction_strategy);
}

#[test]
fn full_precedence_default_then_plugin_then_cli() {
    let plugin = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        proactive_threshold_pct: Some(0.70),
    };

    let cli = CompactionConfig {
        strategy: None,
        proactive_threshold_pct: None,
    };

    let merged = merge_compaction_configs(Some(&plugin), &cli);
    let config = build_compaction_params(&merged);

    assert_eq!(
        config.compaction_strategy,
        CompactionStrategy::SlidingSummary
    );
    assert_eq!(merged.proactive_threshold_pct, Some(0.70));
}

// ── builtin_tool_definitions ─────────────────────────────────────────────────

#[test]
fn builtin_tool_registration_contains_exact_unprefixed_names() {
    let names = builtin_tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "read",
            "edit",
            "patch",
            "skill",
            "http",
            "grep",
            "glob",
            "tmux_session",
            "tmux_window",
            "tmux_pane",
            "tmux_layout",
            "nu",
            "ast_query",
            "ast_nodes",
            "ast_refs",
            "ast_tree"
        ]
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

    // Operation types include both search_replace and create
    let op_types = &edit.parameters["properties"]["operation"]["properties"]["type"]["enum"];
    assert_eq!(op_types[0], "search_replace");
    assert_eq!(op_types[1], "create");

    // Content field exists for create operations
    assert!(
        edit.parameters["properties"]["operation"]["properties"]["content"]["description"]
            .as_str()
            .unwrap_or_default()
            .contains("create")
    );
}

// ── messaging_tool_definitions ───────────────────────────────────────────────

#[test]
fn messaging_tool_registration_contains_only_send_message() {
    let names = messaging_tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["send_message"]);
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
        "send_message description must not reference list_agents, got: {}",
        send.description
    );
}

// ── orchestrator_tool_definitions ────────────────────────────────────────────

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
