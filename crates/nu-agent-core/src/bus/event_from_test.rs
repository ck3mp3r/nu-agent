use super::*;
use crate::protocol::event::ToolDisplay;
use crate::protocol::event::{PermissionDecision, PermissionRequestContext, UiEvent};

#[test]
fn tool_start_converts_to_tool_start() {
    let event = ToolEvent::Start {
        name: "read".to_string(),
        source: "user".to_string(),
        arguments: "{}".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::ToolStart {
            name: "read".to_string(),
            source: "user".to_string(),
            arguments: "{}".to_string(),
        })
    );
}

#[test]
fn tool_end_converts_to_tool_end() {
    let event = ToolEvent::End {
        name: "read".to_string(),
        source: "user".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "ok".to_string(),
        display: None,
        error_kind: None,
        message: None,
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::ToolEnd {
            name: "read".to_string(),
            source: "user".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: "ok".to_string(),
            display: None,
            error_kind: None,
            message: None,
        })
    );
}

#[test]
fn llm_start_converts_to_llm_start() {
    let event = LlmEvent::Start;
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, Some(UiEvent::LlmStart));
}

#[test]
fn llm_end_converts_to_llm_end() {
    let event = LlmEvent::End {
        response_chars: 100,
        tool_calls: 2,
        input_tokens: 10,
        output_tokens: 20,
        total_tokens: 30,
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::LlmEnd {
            response_chars: 100,
            tool_calls: 2,
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
        })
    );
}

#[test]
fn llm_assistant_message_converts() {
    let event = LlmEvent::AssistantMessage {
        text: "hello".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::AssistantMessage {
            text: "hello".to_string(),
        })
    );
}

#[test]
fn warning_message_converts_to_warning() {
    let event = WarningEvent::Message {
        message: "warn".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::Warning {
            message: "warn".to_string(),
        })
    );
}

#[test]
fn warning_turn_error_converts() {
    let event = WarningEvent::TurnError {
        message: "err".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::TurnError {
            message: "err".to_string(),
        })
    );
}

#[test]
fn compaction_started_converts_with_default_source() {
    let event = CompactionEvent::Started { source: None };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::CompactionStarted {
            source: String::new(),
        })
    );
}

#[test]
fn compaction_started_converts_with_source() {
    let event = CompactionEvent::Started {
        source: Some("auto".to_string()),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::CompactionStarted {
            source: "auto".to_string(),
        })
    );
}

#[test]
fn compaction_summary_chunk_converts() {
    let event = CompactionEvent::SummaryChunk {
        source: "auto".to_string(),
        delta: "d".to_string(),
        aggregated: "a".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: "d".to_string(),
            aggregated: "a".to_string(),
        })
    );
}

#[test]
fn compaction_triggered_converts() {
    let event = CompactionEvent::Triggered {
        source: "auto".to_string(),
        summarized_count: 5,
        kept_recent_count: 3,
        summary_preview: "p".to_string(),
        summary_body: "b".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::CompactionTriggered {
            source: "auto".to_string(),
            summarized_count: 5,
            kept_recent_count: 3,
            summary_preview: "p".to_string(),
            summary_body: "b".to_string(),
        })
    );
}

#[test]
fn compaction_failed_converts() {
    let event = CompactionEvent::Failed {
        source: "auto".to_string(),
        message: "fail".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::CompactionFailed {
            source: "auto".to_string(),
            message: "fail".to_string(),
        })
    );
}

#[test]
fn turn_started_dropped() {
    let event = TurnEvent::Started {
        prompt: "hi".to_string(),
        task_id: None,
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);
}

#[test]
fn turn_completed_converts() {
    let event = TurnEvent::TurnCompleted { tool_calls: 3 };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, Some(UiEvent::Completed { tool_calls: 3 }));
}

#[test]
fn turn_task_completed_dropped() {
    let event = TurnEvent::TaskCompleted {
        output: "out".to_string(),
        task_id: "t1".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);
}

#[test]
fn session_started_dropped() {
    let event = SessionEvent::Started {
        session_id: "s1".to_string(),
        hydrated: false,
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);
}

#[test]
fn session_ended_dropped() {
    let event = SessionEvent::Ended {
        session_id: "s1".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);
}

#[test]
fn session_switched_dropped() {
    let event = SessionEvent::Switched {
        from_session_id: None,
        to_session_id: "s2".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);
}

fn make_permission_context() -> PermissionRequestContext {
    PermissionRequestContext {
        tool: "write".to_string(),
        source: "user".to_string(),
        mode: Some("edit".to_string()),
        matched_rule_identity: "identity".to_string(),
        scope: "scope".to_string(),
        target_field: Some("target".to_string()),
        pattern: "pattern".to_string(),
        summary: "summary".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "title".to_string(),
            sections: vec![],
        }),
    }
}

#[test]
fn permission_requested_converts() {
    let context = make_permission_context();
    let event = PermissionEvent::Requested {
        request_id: "r1".to_string(),
        context: Box::new(context.clone()),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::PermissionRequested {
            request_id: "r1".to_string(),
            context,
        })
    );
}

#[test]
fn permission_decision_submitted_converts() {
    let event = PermissionEvent::DecisionSubmitted {
        request_id: "r1".to_string(),
        decision: PermissionDecision::AllowOnce,
        matched_rule_identity: "identity".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::PermissionDecisionSubmitted {
            request_id: "r1".to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "identity".to_string(),
        })
    );
}

#[test]
fn permission_decision_timed_out_converts() {
    let event = PermissionEvent::DecisionTimedOut {
        request_id: "r1".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::PermissionDecisionTimedOut {
            request_id: "r1".to_string(),
        })
    );
}

#[test]
fn permission_decision_ignored_converts() {
    let event = PermissionEvent::DecisionIgnored {
        request_id: "r1".to_string(),
        reason: "auto".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::PermissionDecisionIgnored {
            request_id: "r1".to_string(),
            reason: "auto".to_string(),
        })
    );
}
