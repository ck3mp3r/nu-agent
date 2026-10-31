use super::*;

use crate::session::tool_status::ToolStatus;

// === Tests: event_contract ===

#[test]
fn system_event_contract() {
    let events = [
        SystemEvent::InferenceStarted,
        SystemEvent::Tick,
        SystemEvent::InferenceCompleted {
            usage: Usage {
                response_chars: 12,
                tool_calls: 1,
                input_tokens: 7,
                output_tokens: 5,
                total_tokens: Some(12),
                cost: Some(0.001),
                cost_complete: true,
            },
        },
        SystemEvent::ToolStarted {
            call: ToolCall::new(
                ToolName("k8s__list_pods".to_string()),
                "mcp",
                "{}",
                super::tool_args::CallLine::from_json_summary("{}"),
            ),
        },
        SystemEvent::ToolCompleted {
            call: ToolCall::new(
                ToolName("k8s__list_pods".to_string()),
                "mcp",
                "{}",
                super::tool_args::CallLine::from_json_summary("{}"),
            ),
            outcome: ToolOutcome {
                success: true,
                status: ToolStatus::Success,
                result: "[]".to_string(),
                display: None,
                error_kind: None,
                message: None,
            },
        },
        SystemEvent::PreviewRequested {
            call: ToolCall::new(
                ToolName("nu".to_string()),
                "closure",
                "{\"command\":\"echo hi\"}",
                super::tool_args::CallLine::from_json_summary("{}"),
            ),
            display: crate::protocol::event::ToolDisplay {
                title: Some("nu".to_string()),
                sections: vec![],
            },
        },
        SystemEvent::PermissionRequested {
            request_id: RequestId::new("ask-0000000000000001"),
            context: PermissionRequestContext {
                tool: "nu(command=echo hi)".to_string(),
                tool_key: "nu\n{\"command\":\"echo hi\"}".to_string(),
                source: "closure".to_string(),
                mode: Some("apply".to_string()),
                matched_rule_identity: "nested:nu.command:*".to_string(),
                scope: "nested".to_string(),
                target_field: Some("command".to_string()),
                pattern: "*".to_string(),
                summary: "→ {\"command\":\"echo hi\"}".to_string(),
                pre_authorize_display: None,
            },
        },
        SystemEvent::PermissionDecisionSubmitted {
            request_id: RequestId::new("ask-0000000000000001"),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "nested:nu.command:*".to_string(),
        },
        SystemEvent::PermissionDecisionTimedOut {
            request_id: RequestId::new("ask-0000000000000002"),
        },
        SystemEvent::PermissionDecisionIgnored {
            request_id: RequestId::new("ask-0000000000000003"),
            reason: "stale_or_unknown_request".to_string(),
        },
        SystemEvent::SystemMessage {
            kind: SystemKind::Warning,
            text: "compaction failed".to_string(),
        },
        SystemEvent::CompactionStarted {
            source: "auto_threshold".to_string(),
        },
        SystemEvent::CompactionSummaryChunk {
            source: "auto_threshold".to_string(),
            delta: "chunk".to_string(),
            aggregated: "chunk".to_string(),
        },
        SystemEvent::CompactionCompleted {
            source: "auto_threshold".to_string(),
            summary_preview: "summary preview".to_string(),
            summary_body: "summary body".to_string(),
        },
        SystemEvent::CompactionFailed {
            source: "auto_threshold".to_string(),
            message: "failed".to_string(),
        },
        SystemEvent::InferenceChunk {
            delta: "done".to_string(),
            aggregated: "done".to_string(),
        },
        SystemEvent::TurnCompleted {
            tool_calls: 1,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
    ];

    assert_eq!(events.len(), 17);
}
