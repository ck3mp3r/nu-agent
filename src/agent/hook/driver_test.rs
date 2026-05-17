use super::*;
use crate::agent::protocol::contracts::ProgressUi;
use crate::agent::protocol::event::UiEvent;
use crate::agent::tools::authz::PermissionEventSink;
use crate::agent::tools::handler::McpToolRegistry;
use crate::tools::closure::ClosureRegistry;
use nu_protocol::{BlockId, Span, Spanned, engine::Closure as NuClosure};
use tokio::sync::mpsc;

// Mock ProgressUi that captures events
struct MockUi {
    events: Vec<UiEvent>,
    cancel_requested: bool,
}

impl MockUi {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            cancel_requested: false,
        }
    }

    fn find_tool_start(&self, name: &str) -> Option<&UiEvent> {
        self.events
            .iter()
            .find(|e| matches!(e, UiEvent::ToolStart { name: n, .. } if n == name))
    }

    fn find_tool_end(&self, name: &str) -> Option<&UiEvent> {
        self.events
            .iter()
            .find(|e| matches!(e, UiEvent::ToolEnd { name: n, .. } if n == name))
    }
}

impl ProgressUi for MockUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        self.cancel_requested
    }
}

// Mock permission resolver
struct AllowAll;
impl PermissionResolver for AllowAll {
    fn resolve<S: PermissionEventSink>(
        &mut self,
        _: &str,
        _: &str,
        _: Option<String>,
        _sink: &mut S,
    ) -> PermissionDecision {
        PermissionDecision::Allow
    }
}

struct DenyAll;
impl PermissionResolver for DenyAll {
    fn resolve<S: PermissionEventSink>(
        &mut self,
        _: &str,
        _: &str,
        _: Option<String>,
        _sink: &mut S,
    ) -> PermissionDecision {
        PermissionDecision::Deny
    }
}

/// Helper to create a test closure for ClosureRegistry
fn create_test_closure() -> Spanned<NuClosure> {
    Spanned {
        item: NuClosure {
            block_id: BlockId::new(0),
            captures: vec![],
        },
        span: Span::unknown(),
    }
}

#[test]
fn driver_receives_llm_start_event() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    tx.send(HookEvent::LlmStart).unwrap();
    drop(tx); // Close channel

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );
    assert!(ui.events.iter().any(|e| matches!(e, UiEvent::LlmStart)));
}

#[test]
fn driver_receives_llm_end_event() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    tx.send(HookEvent::LlmEnd {
        response_chars: 100,
        tool_calls: 2,
        input_tokens: 50,
        output_tokens: 75,
        total_tokens: 125,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );
    assert!(
        ui.events
            .iter()
            .any(|e| matches!(e, UiEvent::LlmEnd { .. }))
    );
}

#[test]
fn driver_receives_tool_start_event() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    tx.send(HookEvent::ToolStart {
        name: "read_file".to_string(),
        arguments: "{}".to_string(),
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );
    assert!(
        ui.events
            .iter()
            .any(|e| matches!(e, UiEvent::ToolStart { name, .. } if name == "read_file"))
    );
}

#[test]
fn driver_receives_tool_end_event() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    tx.send(HookEvent::ToolEnd {
        name: "write_file".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "ok".to_string(),
        error_kind: None,
        message: None,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );
    assert!(
        ui.events
            .iter()
            .any(|e| matches!(e, UiEvent::ToolEnd { name, .. } if name == "write_file"))
    );
}

#[test]
fn driver_resolves_permission_allow() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(HookEvent::AskPermission {
        tool_name: "read_file".to_string(),
        arguments: "{}".to_string(),
        tool_call_id: None,
        responder: resp_tx,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // The responder should have been answered
    let decision = resp_rx.blocking_recv().unwrap();
    assert_eq!(decision, PermissionDecision::Allow);
}

#[test]
fn driver_resolves_permission_deny() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = DenyAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(HookEvent::AskPermission {
        tool_name: "write_file".to_string(),
        arguments: "{}".to_string(),
        tool_call_id: None,
        responder: resp_tx,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    let decision = resp_rx.blocking_recv().unwrap();
    assert_eq!(decision, PermissionDecision::Deny);
}

#[test]
fn driver_stops_on_channel_close() {
    let (_tx, rx) = mpsc::unbounded_channel::<HookEvent>();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    drop(_tx); // Immediately close
    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    ); // Should return immediately
    // If this doesn't hang, the test passes
}

#[test]
fn driver_emits_doom_loop_warning() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    tx.send(HookEvent::DoomLoopDetected {
        tool_name: "read_file".to_string(),
        count: 5,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Check that a warning was emitted
    let has_warning = ui
        .events
        .iter()
        .any(|e| matches!(e, UiEvent::Warning { .. }));
    assert!(has_warning);
}

#[test]
fn driver_handles_multiple_events_in_sequence() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    // Send multiple events
    tx.send(HookEvent::LlmStart).unwrap();
    tx.send(HookEvent::ToolStart {
        name: "read".to_string(),
        arguments: "{}".to_string(),
    })
    .unwrap();
    tx.send(HookEvent::ToolEnd {
        name: "read".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "data".to_string(),
        error_kind: None,
        message: None,
    })
    .unwrap();
    tx.send(HookEvent::LlmEnd {
        response_chars: 50,
        tool_calls: 1,
        input_tokens: 20,
        output_tokens: 30,
        total_tokens: 50,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    assert_eq!(ui.events.len(), 4);
    assert!(matches!(ui.events[0], UiEvent::LlmStart));
    assert!(matches!(ui.events[1], UiEvent::ToolStart { ref name, .. } if name == "read"));
    assert!(matches!(ui.events[2], UiEvent::ToolEnd { ref name, .. } if name == "read"));
    assert!(matches!(ui.events[3], UiEvent::LlmEnd { .. }));
}

#[test]
fn driver_fills_tool_source_on_tool_start() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let mut closure_registry = ClosureRegistry::new();
    closure_registry.register("run".to_string(), create_test_closure());
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    // Send ToolStart with empty source (as prompt_hook does)
    tx.send(HookEvent::ToolStart {
        name: "run".to_string(),
        arguments: "{}".to_string(),
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Verify that source was filled by driver using lookup
    let event = ui
        .find_tool_start("run")
        .expect("ToolStart event not found");
    if let UiEvent::ToolStart { source, .. } = event {
        assert_eq!(
            source, "closure",
            "Expected source to be 'closure' for 'run' tool"
        );
    } else {
        panic!("Expected ToolStart event");
    }
}

#[test]
fn driver_fills_tool_source_on_tool_end() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(["mcp_fetch"]);
    let cancel_token = CancellationToken::new();

    // Send ToolEnd (source is resolved by driver)
    tx.send(HookEvent::ToolEnd {
        name: "mcp_fetch".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "ok".to_string(),
        error_kind: None,
        message: None,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Verify that source was filled by driver using lookup
    let event = ui
        .find_tool_end("mcp_fetch")
        .expect("ToolEnd event not found");
    if let UiEvent::ToolEnd { source, .. } = event {
        assert_eq!(
            source, "mcp",
            "Expected source to be 'mcp' for 'mcp_fetch' tool"
        );
    } else {
        panic!("Expected ToolEnd event");
    }
}

#[test]
fn driver_fills_unknown_source_for_unregistered_tool() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    // Send ToolStart for an unknown tool (source resolved by driver)
    tx.send(HookEvent::ToolStart {
        name: "nonexistent".to_string(),
        arguments: "{}".to_string(),
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Verify that source is "unknown" for unregistered tools
    let event = ui
        .find_tool_start("nonexistent")
        .expect("ToolStart event not found");
    if let UiEvent::ToolStart { source, .. } = event {
        assert_eq!(
            source, "unknown",
            "Expected source to be 'unknown' for unregistered tool"
        );
    } else {
        panic!("Expected ToolStart event");
    }
}

#[test]
fn driver_counts_tool_calls() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    // Send multiple ToolEnd events
    tx.send(HookEvent::ToolEnd {
        name: "read".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "data1".to_string(),
        error_kind: None,
        message: None,
    })
    .unwrap();

    tx.send(HookEvent::ToolEnd {
        name: "write".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "data2".to_string(),
        error_kind: None,
        message: None,
    })
    .unwrap();

    tx.send(HookEvent::ToolEnd {
        name: "edit".to_string(),
        arguments: "{}".to_string(),
        success: false,
        result: String::new(),
        error_kind: Some("error".to_string()),
        message: Some("failed".to_string()),
    })
    .unwrap();

    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Driver should count all tool calls (both successful and failed)
    assert_eq!(driver.tool_call_count(), 3);
}

#[test]
fn driver_counts_zero_when_no_tools_called() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    // Send only LLM events, no tool calls
    tx.send(HookEvent::LlmStart).unwrap();
    tx.send(HookEvent::LlmEnd {
        response_chars: 100,
        tool_calls: 0,
        input_tokens: 50,
        output_tokens: 75,
        total_tokens: 125,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // No tool calls should result in count of 0
    assert_eq!(driver.tool_call_count(), 0);
}

// Mock permission resolver that captures the tool_call_id
struct CaptureToolCallId {
    captured_id: Option<String>,
}

impl PermissionResolver for CaptureToolCallId {
    fn resolve<S: PermissionEventSink>(
        &mut self,
        _tool_name: &str,
        _arguments: &str,
        tool_call_id: Option<String>,
        _sink: &mut S,
    ) -> PermissionDecision {
        self.captured_id = tool_call_id;
        PermissionDecision::Allow
    }
}

#[test]
fn driver_passes_tool_call_id_to_permission_resolver() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = CaptureToolCallId { captured_id: None };
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(HookEvent::AskPermission {
        tool_name: "read_file".to_string(),
        arguments: "{}".to_string(),
        tool_call_id: Some("call_abc123".to_string()),
        responder: resp_tx,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Verify the tool_call_id was passed through
    assert_eq!(perms.captured_id, Some("call_abc123".to_string()));

    // Verify permission was granted
    let decision = resp_rx.blocking_recv().unwrap();
    assert_eq!(decision, PermissionDecision::Allow);
}

#[test]
fn driver_passes_none_tool_call_id_when_not_provided() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = CaptureToolCallId {
        captured_id: Some("should_be_cleared".to_string()),
    };
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(HookEvent::AskPermission {
        tool_name: "read_file".to_string(),
        arguments: "{}".to_string(),
        tool_call_id: None,
        responder: resp_tx,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Verify None was passed through
    assert_eq!(perms.captured_id, None);

    // Verify permission was granted
    let decision = resp_rx.blocking_recv().unwrap();
    assert_eq!(decision, PermissionDecision::Allow);
}

#[test]
fn driver_extracts_display_from_edit_tool_result() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    // Simulate edit tool result with display data
    let result_json = serde_json::json!({
        "path": "test.txt",
        "diff": "- old line\n+ new line",
        "stats": {
            "files_changed": 1,
            "insertions": 1,
            "deletions": 1
        }
    });

    tx.send(HookEvent::ToolEnd {
        name: "edit".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: serde_json::to_string(&result_json).unwrap(),
        error_kind: None,
        message: None,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Find the ToolEnd event
    let event = ui.find_tool_end("edit").expect("ToolEnd event not found");
    if let UiEvent::ToolEnd { display, .. } = event {
        assert!(
            display.is_some(),
            "Expected display to be populated for edit tool"
        );
        let display = display.as_ref().unwrap();
        assert_eq!(display.title, "edit test.txt");
        assert_eq!(display.sections.len(), 1);
        assert_eq!(display.sections[0].label, "test.txt");
        assert_eq!(display.sections[0].language, "diff");
        assert_eq!(display.sections[0].content, "- old line\n+ new line");

        let stats = display.sections[0].stats.as_ref().expect("Expected stats");
        assert_eq!(stats.files_changed, Some(1));
        assert_eq!(stats.insertions, Some(1));
        assert_eq!(stats.deletions, Some(1));
    } else {
        panic!("Expected ToolEnd event");
    }
}

#[test]
fn driver_extracts_explicit_display_field() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    // Tool result with explicit display field
    let result_json = serde_json::json!({
        "status": "success",
        "display": {
            "title": "Custom Tool Output",
            "sections": [{
                "label": "Result",
                "language": "json",
                "content": "{\"key\": \"value\"}"
            }]
        }
    });

    tx.send(HookEvent::ToolEnd {
        name: "custom_tool".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: serde_json::to_string(&result_json).unwrap(),
        error_kind: None,
        message: None,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Find the ToolEnd event
    let event = ui
        .find_tool_end("custom_tool")
        .expect("ToolEnd event not found");
    if let UiEvent::ToolEnd { display, .. } = event {
        assert!(
            display.is_some(),
            "Expected display to be populated from explicit display field"
        );
        let display = display.as_ref().unwrap();
        assert_eq!(display.title, "Custom Tool Output");
        assert_eq!(display.sections.len(), 1);
        assert_eq!(display.sections[0].label, "Result");
        assert_eq!(display.sections[0].language, "json");
    } else {
        panic!("Expected ToolEnd event");
    }
}

#[test]
fn driver_leaves_display_none_for_non_displayable_result() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    // Regular JSON result without display data
    let result_json = serde_json::json!({
        "status": "ok",
        "count": 42
    });

    tx.send(HookEvent::ToolEnd {
        name: "some_tool".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: serde_json::to_string(&result_json).unwrap(),
        error_kind: None,
        message: None,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Find the ToolEnd event
    let event = ui
        .find_tool_end("some_tool")
        .expect("ToolEnd event not found");
    if let UiEvent::ToolEnd { display, .. } = event {
        assert!(
            display.is_none(),
            "Expected display to be None for non-displayable results"
        );
    } else {
        panic!("Expected ToolEnd event");
    }
}

#[test]
fn driver_leaves_display_none_for_invalid_json_result() {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut driver = HookDriver {
        event_rx: rx,
        tool_call_count: 0,
    };
    let mut ui = MockUi::new();
    let mut perms = AllowAll;
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names::<[&str; 0], &str>([]);
    let cancel_token = CancellationToken::new();

    tx.send(HookEvent::ToolEnd {
        name: "broken_tool".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "not valid json".to_string(),
        error_kind: None,
        message: None,
    })
    .unwrap();
    drop(tx);

    driver.run_until_complete(
        &mut ui,
        &mut perms,
        &closure_registry,
        &mcp_registry,
        &cancel_token,
    );

    // Find the ToolEnd event
    let event = ui
        .find_tool_end("broken_tool")
        .expect("ToolEnd event not found");
    if let UiEvent::ToolEnd { display, .. } = event {
        assert!(
            display.is_none(),
            "Expected display to be None for invalid JSON"
        );
    } else {
        panic!("Expected ToolEnd event");
    }
}
