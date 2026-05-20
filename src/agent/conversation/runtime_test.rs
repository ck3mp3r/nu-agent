use super::*;

use std::cell::Cell;

use tempfile::tempdir;

use crate::{
    agent::protocol::{compaction::CompactionTriggerSource, contracts::ProgressUi, event::UiEvent},
    agent::tools::handler::McpToolRegistry,
    llm::LlmUsage,
    session::{
        CompactionOutcome, Message, MessageRole, MessageUsage, SessionConfig, SessionStore,
        StoredToolCall,
    },
    tools::mcp::{
        client::McpToolDefinition,
        config::{McpServerConfig, McpTransportType},
    },
};
use rig::completion::message::AssistantContent;

fn persisted_assistant_message(response: &crate::llm::LlmResponse) -> Message {
    let mut msg =
        Message::new(MessageRole::Assistant, response.text.clone()).with_usage(MessageUsage::new(
            response.usage.input_tokens,
            response.usage.output_tokens,
            response.usage.total_tokens,
        ));

    // Convert tool calls to StoredToolCall format if present
    if !response.tool_calls.is_empty() {
        let stored_calls: Vec<StoredToolCall> = response
            .tool_calls
            .iter()
            .filter_map(|content| {
                if let AssistantContent::ToolCall(tc) = content {
                    Some(StoredToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        if !stored_calls.is_empty() {
            msg = msg.with_tool_calls(stored_calls);
        }
    }

    msg
}

fn mcp_tool(server: &str, name: &str, raw_name: &str) -> McpToolDefinition {
    McpToolDefinition {
        server: server.to_string(),
        name: name.to_string(),
        raw_name: raw_name.to_string(),
        description: Some(format!("{server}:{raw_name}")),
        parameters: Some(serde_json::json!({"type":"object"})),
    }
}

fn tool_definition_named(name: &str) -> rig::completion::ToolDefinition {
    rig::completion::ToolDefinition {
        name: name.to_string(),
        description: format!("tool {name}"),
        parameters: serde_json::json!({"type":"object"}),
    }
}

fn mcp_server_config(name: &str, enabled: bool) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport: McpTransportType::Http,
        enabled,
        url: Some("http://localhost:7777/mcp".to_string()),
        headers: std::collections::HashMap::new(),
        command: None,
        cwd: None,
        args: Vec::new(),
        env: std::collections::HashMap::new(),
    }
}

#[derive(Default)]
struct TestProgressUi {
    events: Vec<UiEvent>,
}

impl ProgressUi for TestProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

#[test]
fn persisted_assistant_message_includes_structured_usage_fields() {
    let tmp = tempdir().expect("tempdir");
    let store = SessionStore::new_with_cache_dir(tmp.path().to_path_buf());
    let mut session = store
        .get_or_create(Some("assistant-usage-persist".to_string()))
        .expect("create session");

    let usage = LlmUsage {
        input_tokens: 21,
        output_tokens: 34,
        total_tokens: 55,
        cached_input_tokens: 8,
        cache_creation_input_tokens: 13,
    };

    let response = crate::llm::LlmResponse {
        text: "hello".to_string(),
        usage,
        tool_calls: vec![],
        tool_call_metadata: vec![],
    };

    let assistant = persisted_assistant_message(&response);
    session
        .add_message(&store, assistant)
        .expect("persist message");

    let loaded = store
        .load_session("assistant-usage-persist")
        .expect("load session");
    let persisted = loaded
        .messages()
        .iter()
        .find(|m| m.role() == MessageRole::Assistant)
        .expect("assistant message persisted");

    let persisted_usage = persisted.usage().expect("assistant usage persisted");
    assert_eq!(persisted_usage.input_tokens(), Some(21));
    assert_eq!(persisted_usage.output_tokens(), Some(34));
    assert_eq!(persisted_usage.total_tokens(), Some(55));
}

#[test]
fn manual_and_auto_compaction_share_single_execution_path() {
    let mut ui = TestProgressUi::default();
    let counter = Cell::new(0usize);

    let manual = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
        counter.set(counter.get() + 1);
        Ok(Some(CompactionOutcome {
            summarized_count: 1,
            kept_recent_count: 1,
            summary_text: "summary".to_string(),
        }))
    });
    if let Ok(event) = &manual {
        ui.emit(event);
    }
    let auto = execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
        counter.set(counter.get() + 1);
        Ok(Some(CompactionOutcome {
            summarized_count: 1,
            kept_recent_count: 1,
            summary_text: "summary".to_string(),
        }))
    });
    if let Ok(event) = &auto {
        ui.emit(event);
    }

    assert!(manual.is_ok());
    assert!(auto.is_ok());
    assert_eq!(counter.get(), 2);
}

#[test]
fn compaction_event_emits_correct_source_metadata() {
    let mut ui = TestProgressUi::default();

    execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
        Ok(Some(CompactionOutcome {
            summarized_count: 3,
            kept_recent_count: 2,
            summary_text: "auto summary body".to_string(),
        }))
    })
    .map(|event| ui.emit(&event))
    .expect("auto event");
    execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
        Ok(Some(CompactionOutcome {
            summarized_count: 4,
            kept_recent_count: 1,
            summary_text: "manual summary body".to_string(),
        }))
    })
    .map(|event| ui.emit(&event))
    .expect("manual event");

    assert!(ui.events.contains(&UiEvent::CompactionTriggered {
        source: "auto_threshold".to_string(),
        summarized_count: 3,
        kept_recent_count: 2,
        summary_preview: "auto summary body".to_string(),
        summary_body: "auto summary body".to_string(),
    }));
    assert!(ui.events.contains(&UiEvent::CompactionTriggered {
        source: "slash_compact".to_string(),
        summarized_count: 4,
        kept_recent_count: 1,
        summary_preview: "manual summary body".to_string(),
        summary_body: "manual summary body".to_string(),
    }));
}

#[test]
fn compaction_summary_transcript_includes_source_and_counts() {
    let mut ui = TestProgressUi::default();

    execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
        Ok(Some(CompactionOutcome {
            summarized_count: 7,
            kept_recent_count: 3,
            summary_text: "summary body for transcript".to_string(),
        }))
    })
    .map(|event| ui.emit(&event))
    .expect("event");

    assert!(ui.events.contains(&UiEvent::CompactionTriggered {
        source: "auto_threshold".to_string(),
        summarized_count: 7,
        kept_recent_count: 3,
        summary_preview: "summary body for transcript".to_string(),
        summary_body: "summary body for transcript".to_string(),
    }));
}

#[test]
fn sliding_summary_compaction_failure_warning_text_is_source_consistent() {
    let tmp = tempdir().expect("tempdir");
    let store = SessionStore::new_with_cache_dir(tmp.path().to_path_buf());
    let mut session = store
        .get_or_create(Some("failure-warning-consistent".to_string()))
        .expect("create session");
    session.set_config(SessionConfig {
        compaction_threshold: 1,
        keep_recent: 1,
        ..SessionConfig::default()
    });
    session
        .add_message(&store, Message::new(MessageRole::User, "a".to_string()))
        .expect("message");
    session
        .add_message(
            &store,
            Message::new(MessageRole::Assistant, "b".to_string()),
        )
        .expect("message");

    let manual = execute_compaction_persisted(
        &mut session,
        &store,
        |_old| Err(std::io::Error::other("manual-source-failure")),
        crate::session::CompactionInvocationMode::Force,
    );
    let auto = execute_compaction_persisted(
        &mut session,
        &store,
        |_old| Err(std::io::Error::other("auto-source-failure")),
        crate::session::CompactionInvocationMode::Threshold,
    );

    assert_eq!(
        manual.expect_err("manual error"),
        COMPACTION_FAILURE_WARNING.to_string()
    );
    assert_eq!(
        auto.expect_err("auto error"),
        COMPACTION_FAILURE_WARNING.to_string()
    );
}

#[test]
fn manual_and_auto_compaction_failure_surface_is_consistent() {
    let manual = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
        Err("Session compaction failed: disk full".to_string())
    });
    let auto = execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
        Err("Session compaction failed: disk full".to_string())
    });

    assert_eq!(manual, auto);
}

#[test]
fn manual_compaction_persists_session_file_updates() {
    let tmp = tempdir().expect("tempdir");
    let store = SessionStore::new_with_cache_dir(tmp.path().to_path_buf());
    let mut session = store
        .get_or_create(Some("manual-compact-persists".to_string()))
        .expect("create session");
    session.set_config(SessionConfig {
        compaction_threshold: 2,
        keep_recent: 1,
        ..SessionConfig::default()
    });

    session
        .add_message(&store, Message::new(MessageRole::User, "a".to_string()))
        .expect("message");
    session
        .add_message(
            &store,
            Message::new(MessageRole::Assistant, "b".to_string()),
        )
        .expect("message");
    session
        .add_message(&store, Message::new(MessageRole::User, "c".to_string()))
        .expect("message");

    let mut ui = TestProgressUi::default();
    execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
        execute_compaction_persisted(
            &mut session,
            &store,
            |_old| Ok("summary".to_string()),
            crate::session::CompactionInvocationMode::Force,
        )
    })
    .map(|event| ui.emit(&event))
    .expect("manual compaction");

    let loaded = store
        .load_session("manual-compact-persists")
        .expect("reload session");
    assert!(loaded.compaction_count() > 0);
}

#[test]
fn auto_compaction_persists_session_file_updates() {
    let tmp = tempdir().expect("tempdir");
    let store = SessionStore::new_with_cache_dir(tmp.path().to_path_buf());
    let mut session = store
        .get_or_create(Some("auto-compact-persists".to_string()))
        .expect("create session");
    session.set_config(SessionConfig {
        compaction_threshold: 2,
        keep_recent: 1,
        ..SessionConfig::default()
    });

    session
        .add_message(&store, Message::new(MessageRole::User, "a".to_string()))
        .expect("message");
    session
        .add_message(
            &store,
            Message::new(MessageRole::Assistant, "b".to_string()),
        )
        .expect("message");
    session
        .add_message(&store, Message::new(MessageRole::User, "c".to_string()))
        .expect("message");

    let mut ui = TestProgressUi::default();
    execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
        execute_compaction_persisted(
            &mut session,
            &store,
            |_old| Ok("summary".to_string()),
            crate::session::CompactionInvocationMode::Threshold,
        )
    })
    .map(|event| ui.emit(&event))
    .expect("auto compaction");

    let loaded = store
        .load_session("auto-compact-persists")
        .expect("reload session");
    assert!(loaded.compaction_count() > 0);
}

#[test]
fn permissions_startup_summary_emits_once_before_first_turn() {
    let mut ui = TestProgressUi::default();
    let mut emitted = false;
    let summary =
        "permissions policy: overlay_active=false global=ask tool_rules=5 nu__run.command_rules=1";

    emit_permissions_startup_summary_once(&mut ui, &mut emitted, summary);
    emit_permissions_startup_summary_once(&mut ui, &mut emitted, summary);

    let warnings = ui
        .events
        .iter()
        .filter(|e| matches!(e, UiEvent::Warning { .. }))
        .count();
    assert_eq!(warnings, 1);

    let warning_message = ui
        .events
        .iter()
        .find_map(|event| match event {
            UiEvent::Warning { message } => Some(message.clone()),
            _ => None,
        })
        .expect("warning event");
    assert_eq!(warning_message, summary);
}

#[test]
fn enabling_startup_disabled_server_materializes_filtered_mcp_tools_for_current_session() {
    let mut tool_definitions = vec![tool_definition_named("read")];
    let mut registry =
        McpToolRegistry::from_tools(vec![mcp_tool("gh", "gh__list_prs", "list_prs")])
            .expect("startup registry");

    let discovered_from_toggle = vec![
        mcp_tool("k8s", "k8s__list_pods", "list_pods"),
        mcp_tool("k8s", "k8s__delete_pod", "delete_pod"),
    ];

    super::merge_new_mcp_tools_into_runtime(
        &mut tool_definitions,
        &mut registry,
        &discovered_from_toggle,
        &["k8s__list_*".to_string()],
    )
    .expect("toggle merge should succeed");

    let visible =
        crate::agent::tools::handler::llm_visible_tool_definitions(&tool_definitions, &registry);

    assert!(visible.iter().any(|tool| tool.name == "k8s__list_pods"));
    assert!(
        visible.iter().all(|tool| tool.name != "k8s__delete_pod"),
        "cli MCP patterns must be applied consistently when enabling servers in-session"
    );
    assert_eq!(
        visible
            .iter()
            .filter(|tool| tool.name.starts_with("k8s__"))
            .count(),
        1
    );
}

#[test]
fn enabling_startup_disabled_server_registers_dispatch_raw_name_mapping() {
    let mut tool_definitions = vec![tool_definition_named("read")];
    let mut registry = McpToolRegistry::from_names(Vec::<String>::new());

    let discovered = vec![mcp_tool("k8s", "k8s__list_pods", "list_pods")];

    super::merge_new_mcp_tools_into_runtime(&mut tool_definitions, &mut registry, &discovered, &[])
        .expect("toggle merge should succeed");

    assert_eq!(registry.raw_name_for("k8s__list_pods"), Some("list_pods"));
    assert!(registry.contains("k8s__list_pods"));
}

#[test]
fn enabling_stage_conflict_is_transactional_and_keeps_original_runtime_state() {
    let tool_definitions = vec![tool_definition_named("read")];
    let registry = McpToolRegistry::from_tools(vec![mcp_tool("gh", "k8s__list_pods", "list_pods")])
        .expect("startup registry");

    let discovered_conflict = vec![mcp_tool("k8s", "k8s__list_pods", "list_all_pods")];

    let result = stage_enabled_mcp_runtime_state(
        &tool_definitions,
        &registry,
        "k8s",
        &discovered_conflict,
        &[],
    );

    assert!(result.is_err());
    assert!(
        result
            .expect_err("must fail on conflicting raw mapping")
            .contains("conflicting raw MCP tool mapping")
    );

    assert_eq!(
        tool_definitions.len(),
        1,
        "tool definitions must remain unchanged"
    );
    assert_eq!(tool_definitions[0].name, "read");

    assert!(
        registry.contains("k8s__list_pods"),
        "existing registry mapping must remain visible"
    );
    assert_eq!(registry.raw_name_for("k8s__list_pods"), Some("list_pods"));
    assert!(
        !registry.is_server_enabled("k8s"),
        "new server must not be enabled on conflict"
    );
}

#[test]
fn lifecycle_projection_recomputes_from_registry_state_without_runtime() {
    let registry = McpToolRegistry::from_tools(vec![
        mcp_tool("gh", "gh__list_prs", "list_prs"),
        mcp_tool("k8s", "k8s__list_pods", "list_pods"),
    ])
    .expect("registry");
    registry
        .set_server_enabled("k8s", false)
        .expect("disable k8s");

    let configs = vec![
        mcp_server_config("k8s", true),
        mcp_server_config("gh", true),
    ];

    let projection = super::rebuild_mcp_lifecycle_projection(
        None,
        &configs,
        &registry,
        &[
            tool_definition_named("read"),
            tool_definition_named("gh__list_prs"),
            tool_definition_named("k8s__list_pods"),
        ],
    );

    assert_eq!(projection.len(), 2);
    assert_eq!(projection[0].name, "gh");
    assert!(projection[0].enabled);
    assert!(!projection[0].connected);
    assert_eq!(projection[0].visible_tool_count, 1);

    assert_eq!(projection[1].name, "k8s");
    assert!(!projection[1].enabled);
    assert!(!projection[1].connected);
    assert_eq!(projection[1].visible_tool_count, 0);
}

// ========================================================================
// Structured messages tests
// ========================================================================

#[test]
fn build_system_preamble_joins_non_empty_parts() {
    let result = super::build_system_preamble(
        Some("preamble text"),
        Some("context text"),
        Some("agents chain"),
        Some("available skills"),
    );

    assert!(result.is_some());
    let text = result.unwrap();
    assert!(text.contains("preamble text"));
    assert!(text.contains("context text"));
    assert!(text.contains("agents chain"));
    assert!(text.contains("available skills"));
}

#[test]
fn build_system_preamble_returns_none_when_all_empty() {
    let result = super::build_system_preamble(None, None, None, None);
    assert!(result.is_none());
}

#[test]
fn build_system_preamble_handles_partial_inputs() {
    let result = super::build_system_preamble(Some("preamble"), None, Some("agents"), None);

    assert!(result.is_some());
    let text = result.unwrap();
    assert!(text.contains("preamble"));
    assert!(text.contains("agents"));
}
