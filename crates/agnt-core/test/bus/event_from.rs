use super::*;
use crate::protocol::event::ToolDisplay;
use crate::protocol::event::{PermissionDecision, PermissionRequestContext, UiEvent};
use crate::protocol::tool_args::CallLine;

/// Tool and LLM events convert to their UI counterparts.
#[test]
fn tool_and_llm_events_convert() {
    // -- Case 1: ToolEvent::Started
    let event = ToolEvent::Started {
        name: "read".to_string(),
        source: "user".to_string(),
        arguments: "{}".to_string(),
        call_line: CallLine::from_json_summary("{}"),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::ToolStarted {
            name: "read".to_string(),
            source: "user".to_string(),
            arguments: "{}".to_string(),
            call_line: CallLine::from_json_summary("{}"),
        })
    );

    // -- Case 2: ToolEvent::Completed
    let event = ToolEvent::Completed {
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
        Some(UiEvent::ToolCompleted {
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

    // -- Case 3: LlmEvent::Started
    let ui: Option<UiEvent> = LlmEvent::Started.into();
    assert_eq!(ui, Some(UiEvent::LlmStarted));

    // -- Case 4: LlmEvent::Completed
    let event = LlmEvent::Completed {
        response_chars: 100,
        tool_calls: 2,
        input_tokens: 10,
        output_tokens: 20,
        total_tokens: Some(30),
        cost: Some(0.012),
        cost_complete: true,
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::LlmCompleted {
            response_chars: 100,
            tool_calls: 2,
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: Some(30),
            cost: Some(0.012),
            cost_complete: true,
        })
    );

    // -- Case 5: LlmEvent::AssistantMessage
    let event = LlmEvent::AssistantMessage {
        text: "hello".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::AssistantMessage {
            delta: "hello".to_string(),
            text: "hello".to_string(),
        })
    );
}

/// Warning and compaction events convert to their UI counterparts, except
/// `CompactionEvent::Requested`, which does not render.
#[test]
fn warning_and_compaction_events_convert() {
    // -- Case 1: WarningEvent::Message
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

    // -- Case 2: WarningEvent::TurnError
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

    // -- Case 3: CompactionEvent::Requested does not render
    let event = CompactionEvent::Requested {
        source: "auto".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui, None,
        "CompactionEvent::Requested must not render to a UiEvent"
    );

    // -- Case 4: CompactionEvent::Started
    let event = CompactionEvent::Started {
        source: "auto".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::CompactionStarted {
            source: "auto".to_string(),
        })
    );

    // -- Case 5: CompactionEvent::SummaryChunk
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

    // -- Case 6: CompactionEvent::Completed
    let event = CompactionEvent::Completed {
        source: "auto".to_string(),
        summary_preview: "p".to_string(),
        summary_body: "b".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(
        ui,
        Some(UiEvent::CompactionCompleted {
            source: "auto".to_string(),
            summary_preview: "p".to_string(),
            summary_body: "b".to_string(),
        })
    );

    // -- Case 7: CompactionEvent::Failed
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

/// Turn and session events do not render to the UI, except
/// `TurnEvent::Completed`.
#[test]
fn turn_and_session_events_do_not_render() {
    // -- Case 1: TurnEvent::Started does not render
    let event = TurnEvent::Started {
        prompt: "hi".to_string(),
        task_id: None,
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);

    // -- Case 2: TurnEvent::Completed renders
    let event = TurnEvent::Completed { tool_calls: 3 };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, Some(UiEvent::Completed { tool_calls: 3 }));

    // -- Case 3: TurnEvent::TaskCompleted does not render
    let event = TurnEvent::TaskCompleted {
        output: "out".to_string(),
        task_id: "t1".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);

    // -- Case 4: TurnEvent::TaskFailed does not render
    let event = TurnEvent::TaskFailed {
        task_id: "t1".to_string(),
        error: "boom".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);

    // -- Case 5: SessionEvent::Started does not render
    let event = SessionEvent::Started {
        session_id: "s1".to_string(),
        hydrated: false,
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);

    // -- Case 6: SessionEvent::Ended does not render
    let event = SessionEvent::Ended {
        session_id: "s1".to_string(),
    };
    let ui: Option<UiEvent> = event.into();
    assert_eq!(ui, None);

    // -- Case 7: SessionEvent::Switched does not render
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
        tool_key: "write\n{}".to_string(),
        source: "user".to_string(),
        mode: Some("edit".to_string()),
        matched_rule_identity: "identity".to_string(),
        scope: "scope".to_string(),
        target_field: Some("target".to_string()),
        pattern: "pattern".to_string(),
        summary: "summary".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: Some("title".to_string()),
            sections: vec![],
        }),
    }
}

/// Permission events convert to their UI counterparts.
#[test]
fn permission_events_convert() {
    // -- Case 1: PermissionEvent::Requested
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

    // -- Case 2: PermissionEvent::DecisionSubmitted
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

    // -- Case 3: PermissionEvent::DecisionTimedOut
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

    // -- Case 4: PermissionEvent::DecisionIgnored
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
