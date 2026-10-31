//! Vocabulary tests for `SystemEvent` (protocol/events.rs).

use crate::protocol::event::{PermissionDecision, PermissionRequestContext, ToolDisplay};
use crate::protocol::events::{
    AutoResolutionAction, Key, RequestId, SystemEvent, SystemKind, ToolCall, ToolCallId,
    ToolOutcome, Usage,
};
use crate::protocol::tool_args::CallLine;
use crate::session::tool_status::ToolStatus;
use crate::transcript::ir::ToolName;

// region:    --- Tests

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_protocol_events_tool_call_id_of_derives_call_key() -> Result<()> {
    // -- Exec
    let id = ToolCallId::of("nu", "{\"command\":\"ls\"}");
    let other = ToolCallId::of("nu", "{\"command\":\"ls -la\"}");

    // -- Check
    assert_eq!(id.as_str(), "nu\n{\"command\":\"ls\"}");
    assert_ne!(id.as_str(), other.as_str());
    Ok(())
}

#[test]
fn test_protocol_events_tool_call_new_derives_id_from_payload() -> Result<()> {
    // -- Setup & Fixtures
    let arguments = "{\"path\":\"src/main.rs\"}";

    // -- Exec
    let call = ToolCall::new(
        ToolName("edit".to_string()),
        "builtin",
        arguments,
        CallLine {
            summary: "\u{2192} edit src/main.rs".to_string(),
        },
    );

    // -- Check
    assert_eq!(call.id, ToolCallId::of("edit", arguments));
    assert_eq!(call.name, ToolName("edit".to_string()));
    assert_eq!(call.source, "builtin");
    assert_eq!(call.arguments, arguments);
    assert_eq!(call.call_line.summary, "\u{2192} edit src/main.rs");
    Ok(())
}

#[test]
fn test_protocol_events_vocabulary_covers_completeness_trace() -> Result<()> {
    // -- Setup & Fixtures
    let call = tool_call();
    let display = ToolDisplay {
        title: Some("edit src/main.rs".to_string()),
        sections: Vec::new(),
    };
    let request_id = RequestId::new("req-1");

    let events = vec![
        SystemEvent::KeyPressed {
            key: Key::Char('a'),
        },
        SystemEvent::WheelScrolled {
            up: true,
            column: 3,
            row: 7,
        },
        SystemEvent::Resized {
            width: 120,
            height: 40,
        },
        SystemEvent::PermissionDecided {
            decision: PermissionDecision::AllowOnce,
        },
        SystemEvent::PromptSubmitted {
            text: "hello".to_string(),
        },
        SystemEvent::InferenceStarted,
        SystemEvent::InferenceChunk {
            delta: "hi".to_string(),
            aggregated: "hi".to_string(),
        },
        SystemEvent::InferenceCompleted {
            usage: Usage {
                response_chars: 2,
                tool_calls: 1,
                input_tokens: 10,
                output_tokens: 2,
                total_tokens: Some(12),
                cost: Some(0.001),
                cost_complete: true,
            },
        },
        SystemEvent::InferenceStopped {
            reason: "hook".to_string(),
        },
        SystemEvent::ToolStarted { call: call.clone() },
        SystemEvent::PreviewRequested {
            call: call.clone(),
            display: display.clone(),
        },
        SystemEvent::ToolCompleted {
            call: call.clone(),
            outcome: ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: "{}".to_string(),
                display: Some(display),
                error_kind: None,
                message: None,
            },
        },
        SystemEvent::PermissionRequested {
            request_id: request_id.clone(),
            context: permission_context(),
        },
        SystemEvent::PermissionDecisionSubmitted {
            request_id: request_id.clone(),
            decision: PermissionDecision::Deny,
            matched_rule_identity: "rule-1".to_string(),
        },
        SystemEvent::PermissionDecisionTimedOut {
            request_id: request_id.clone(),
        },
        SystemEvent::PermissionDecisionIgnored {
            request_id: request_id.clone(),
            reason: "turn cancelled".to_string(),
        },
        SystemEvent::PermissionRequestCancelled {
            request_id,
            matched_rule_identity: "rule-1".to_string(),
        },
        SystemEvent::PermissionAutoResolved {
            matched_rule_identity: "rule-1".to_string(),
            action: AutoResolutionAction::Allow,
            reason: None,
        },
        SystemEvent::SystemMessage {
            kind: SystemKind::Warning,
            text: "careful".to_string(),
        },
        SystemEvent::CompactionRequested {
            source: "auto".to_string(),
        },
        SystemEvent::CompactionStarted {
            source: "auto".to_string(),
        },
        SystemEvent::CompactionSummaryChunk {
            source: "auto".to_string(),
            delta: "sum".to_string(),
            aggregated: "sum".to_string(),
        },
        SystemEvent::CompactionCompleted {
            source: "auto".to_string(),
            summary_preview: "sum".to_string(),
            summary_body: "sum".to_string(),
        },
        SystemEvent::CompactionFailed {
            source: "auto".to_string(),
            message: "boom".to_string(),
        },
        SystemEvent::TurnCompleted {
            tool_calls: 1,
            input_tokens: 10,
            output_tokens: 2,
            total_tokens: Some(12),
            cost: Some(0.001),
            cost_complete: true,
            duration_ms: 1500,
        },
        SystemEvent::Tick,
    ];

    // -- Exec
    let count = events.len();

    // -- Check
    assert_eq!(count, 26);
    Ok(())
}

#[test]
fn test_protocol_events_payloads_carry_typed_identity() -> Result<()> {
    // -- Setup & Fixtures
    let call = tool_call();

    // -- Exec
    let started = SystemEvent::ToolStarted { call: call.clone() };
    let auto_resolved = SystemEvent::PermissionAutoResolved {
        matched_rule_identity: "rule-1".to_string(),
        action: AutoResolutionAction::Deny,
        reason: Some("blocked by policy".to_string()),
    };
    let completed = SystemEvent::TurnCompleted {
        tool_calls: 3,
        input_tokens: 100,
        output_tokens: 20,
        total_tokens: None,
        cost: None,
        cost_complete: false,
        duration_ms: 4200,
    };

    // -- Check
    let SystemEvent::ToolStarted { call: started_call } = started else {
        return Err("should be ToolStarted".into());
    };
    assert_eq!(started_call.id.as_str(), "nu\n{\"command\":\"ls\"}");

    let SystemEvent::PermissionAutoResolved {
        action,
        reason,
        matched_rule_identity,
    } = auto_resolved
    else {
        return Err("should be PermissionAutoResolved".into());
    };
    assert_eq!(action, AutoResolutionAction::Deny);
    assert_eq!(reason.as_deref(), Some("blocked by policy"));
    assert_eq!(matched_rule_identity, "rule-1");

    let SystemEvent::TurnCompleted {
        tool_calls,
        total_tokens,
        cost,
        duration_ms,
        ..
    } = completed
    else {
        return Err("should be TurnCompleted".into());
    };
    assert_eq!(tool_calls, 3);
    assert_eq!(total_tokens, None);
    assert_eq!(cost, None);
    assert_eq!(duration_ms, 4200);
    Ok(())
}

// endregion: --- Tests

// region:    --- Test Support

fn tool_call() -> ToolCall {
    ToolCall::new(
        ToolName("nu".to_string()),
        "builtin",
        "{\"command\":\"ls\"}",
        CallLine {
            summary: "\u{2192} nu ls".to_string(),
        },
    )
}

fn permission_context() -> PermissionRequestContext {
    PermissionRequestContext {
        tool: "nu".to_string(),
        tool_key: "nu\n{\"command\":\"ls\"}".to_string(),
        source: "builtin".to_string(),
        mode: None,
        matched_rule_identity: "rule-1".to_string(),
        scope: "call".to_string(),
        target_field: None,
        pattern: "nu".to_string(),
        summary: "run nu".to_string(),
        pre_authorize_display: None,
    }
}

// endregion: --- Test Support
