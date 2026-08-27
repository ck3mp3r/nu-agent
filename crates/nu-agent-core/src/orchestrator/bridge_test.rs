use crate::bus::PermissionEvent;
use crate::orchestrator::bridge::{BridgeAction, bridge_action};
use crate::protocol::event::{PermissionDecision, PermissionRequestContext, UiEvent};

// ---------------------------------------------------------------------------
// Lifecycle events must NOT be re-published by the worker bridge.
// ---------------------------------------------------------------------------

#[test]
fn tool_start_is_ignored() {
    let event = UiEvent::ToolStarted {
        name: "shell".into(),
        source: "builtin".into(),
        arguments: "{}".into(),
    };
    assert!(matches!(bridge_action(event), BridgeAction::Ignore));
}

#[test]
fn tool_end_is_ignored() {
    let event = UiEvent::ToolCompleted {
        name: "shell".into(),
        source: "builtin".into(),
        arguments: "{}".into(),
        success: true,
        result: "ok".into(),
        display: None,
        error_kind: None,
        message: None,
    };
    assert!(matches!(bridge_action(event), BridgeAction::Ignore));
}

#[test]
fn llm_start_is_ignored() {
    assert!(matches!(
        bridge_action(UiEvent::LlmStarted),
        BridgeAction::Ignore
    ));
}

#[test]
fn llm_end_is_ignored() {
    let event = UiEvent::LlmCompleted {
        response_chars: 10,
        tool_calls: 1,
        input_tokens: 5,
        output_tokens: 10,
        total_tokens: 15,
    };
    assert!(matches!(bridge_action(event), BridgeAction::Ignore));
}

#[test]
fn assistant_message_is_ignored() {
    let event = UiEvent::AssistantMessage {
        text: "hello".into(),
    };
    assert!(matches!(bridge_action(event), BridgeAction::Ignore));
}

#[test]
fn warning_is_ignored() {
    let event = UiEvent::Warning {
        message: "context".into(),
    };
    assert!(matches!(bridge_action(event), BridgeAction::Ignore));
}

#[test]
fn turn_error_is_ignored() {
    let event = UiEvent::TurnError {
        message: "boom".into(),
    };
    assert!(matches!(bridge_action(event), BridgeAction::Ignore));
}

#[test]
fn compaction_events_are_ignored() {
    let started = UiEvent::CompactionStarted {
        source: "manual".into(),
    };
    let chunk = UiEvent::CompactionSummaryChunk {
        source: "manual".into(),
        delta: "d".into(),
        aggregated: "a".into(),
    };
    let triggered = UiEvent::CompactionCompleted {
        source: "manual".into(),
        summary_preview: "p".into(),
        summary_body: "b".into(),
    };
    let failed = UiEvent::CompactionFailed {
        source: "manual".into(),
        message: "m".into(),
    };
    assert!(matches!(bridge_action(started), BridgeAction::Ignore));
    assert!(matches!(bridge_action(chunk), BridgeAction::Ignore));
    assert!(matches!(bridge_action(triggered), BridgeAction::Ignore));
    assert!(matches!(bridge_action(failed), BridgeAction::Ignore));
}

#[test]
fn completed_is_ignored() {
    let event = UiEvent::Completed { tool_calls: 2 };
    assert!(matches!(bridge_action(event), BridgeAction::Ignore));
}

#[test]
fn tick_is_ignored() {
    assert!(matches!(bridge_action(UiEvent::Tick), BridgeAction::Ignore));
}

// ---------------------------------------------------------------------------
// Permission events must still be published to the permission bus channel.
// ---------------------------------------------------------------------------

#[test]
fn permission_requested_is_published() {
    let event = UiEvent::PermissionRequested {
        request_id: "req-1".into(),
        context: PermissionRequestContext {
            tool: "shell".into(),
            source: "builtin".into(),
            mode: None,
            matched_rule_identity: String::new(),
            scope: String::new(),
            target_field: None,
            pattern: "*".into(),
            summary: String::new(),
            pre_authorize_display: None,
        },
    };
    match bridge_action(event) {
        BridgeAction::PublishPermission(PermissionEvent::Requested {
            request_id,
            context,
        }) => {
            assert_eq!(request_id, "req-1");
            assert_eq!(context.tool, "shell");
        }
        other => panic!("expected PermissionEvent::Requested, got {other:?}"),
    }
}

#[test]
fn permission_decision_submitted_is_published() {
    let event = UiEvent::PermissionDecisionSubmitted {
        request_id: "req-1".into(),
        decision: PermissionDecision::AllowOnce,
        matched_rule_identity: "rule-1".into(),
    };
    match bridge_action(event) {
        BridgeAction::PublishPermission(PermissionEvent::DecisionSubmitted {
            request_id,
            decision,
            matched_rule_identity,
        }) => {
            assert_eq!(request_id, "req-1");
            assert_eq!(decision, PermissionDecision::AllowOnce);
            assert_eq!(matched_rule_identity, "rule-1");
        }
        other => panic!("expected PermissionEvent::DecisionSubmitted, got {other:?}"),
    }
}

#[test]
fn permission_decision_timed_out_is_published() {
    let event = UiEvent::PermissionDecisionTimedOut {
        request_id: "req-1".into(),
    };
    match bridge_action(event) {
        BridgeAction::PublishPermission(PermissionEvent::DecisionTimedOut { request_id }) => {
            assert_eq!(request_id, "req-1");
        }
        other => panic!("expected PermissionEvent::DecisionTimedOut, got {other:?}"),
    }
}

#[test]
fn permission_decision_ignored_is_published() {
    let event = UiEvent::PermissionDecisionIgnored {
        request_id: "req-1".into(),
        reason: "user".into(),
    };
    match bridge_action(event) {
        BridgeAction::PublishPermission(PermissionEvent::DecisionIgnored {
            request_id,
            reason,
        }) => {
            assert_eq!(request_id, "req-1");
            assert_eq!(reason, "user");
        }
        other => panic!("expected PermissionEvent::DecisionIgnored, got {other:?}"),
    }
}

// Keep ToolDisplay import used to satisfy dead-code checks if needed.
