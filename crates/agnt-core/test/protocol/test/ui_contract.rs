use super::*;

// === Tests: event_contract ===

#[test]
fn ui_event_and_permission_contracts() {
    let events = [
        UiEvent::LlmStarted,
        UiEvent::Tick,
        UiEvent::LlmCompleted {
            response_chars: 12,
            tool_calls: 1,
            input_tokens: 7,
            output_tokens: 5,
            total_tokens: 12,
        },
        UiEvent::ToolStarted {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            call_line: super::tool_args::CallLine::from_json_summary("{}"),
        },
        UiEvent::ToolCompleted {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: "[]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
        UiEvent::ToolPreview {
            tool_key: "nu\n{\"command\":\"echo hi\"}".to_string(),
            display: crate::protocol::event::ToolDisplay {
                title: Some("nu".to_string()),
                sections: vec![],
            },
        },
        UiEvent::PermissionRequested {
            request_id: "ask-0000000000000001".to_string(),
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
        UiEvent::PermissionDecisionSubmitted {
            request_id: "ask-0000000000000001".to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "nested:nu.command:*".to_string(),
        },
        UiEvent::PermissionDecisionTimedOut {
            request_id: "ask-0000000000000002".to_string(),
        },
        UiEvent::PermissionDecisionIgnored {
            request_id: "ask-0000000000000003".to_string(),
            reason: "stale_or_unknown_request".to_string(),
        },
        UiEvent::Warning {
            message: "compaction failed".to_string(),
        },
        UiEvent::CompactionStarted {
            source: "auto_threshold".to_string(),
        },
        UiEvent::CompactionSummaryChunk {
            source: "auto_threshold".to_string(),
            delta: "chunk".to_string(),
            aggregated: "chunk".to_string(),
        },
        UiEvent::CompactionCompleted {
            source: "auto_threshold".to_string(),
            summary_preview: "summary preview".to_string(),
            summary_body: "summary body".to_string(),
        },
        UiEvent::CompactionFailed {
            source: "auto_threshold".to_string(),
            message: "failed".to_string(),
        },
        UiEvent::AssistantMessage {
            delta: "done".to_string(),
            text: "done".to_string(),
        },
        UiEvent::Completed { tool_calls: 1 },
    ];

    assert_eq!(events.len(), 17);

    // The permission event field shape is explicit and stable.
    let requested = PermissionEvent::Requested {
        request_id: "ask-0000000000000001".to_string(),
        context: Box::new(PermissionRequestContext {
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
        }),
    };
    match requested {
        PermissionEvent::Requested {
            request_id,
            context,
        } => {
            assert_eq!(request_id, "ask-0000000000000001");
            assert_eq!(context.tool, "nu(command=echo hi)");
            assert_eq!(context.source, "closure");
            assert_eq!(context.mode.as_deref(), Some("apply"));
            assert_eq!(context.matched_rule_identity, "nested:nu.command:*");
            assert_eq!(context.scope, "nested");
            assert_eq!(context.target_field.as_deref(), Some("command"));
            assert_eq!(context.pattern, "*");
            assert!(context.summary.starts_with("→ "));
            assert!(context.pre_authorize_display.is_none());
        }
        other => panic!("unexpected variant: {other:?}"),
    }

    let submitted = PermissionEvent::DecisionSubmitted {
        request_id: "ask-0000000000000001".to_string(),
        decision: PermissionDecision::AllowAlways,
        matched_rule_identity: "nested:nu.command:*".to_string(),
    };
    match submitted {
        PermissionEvent::DecisionSubmitted {
            request_id,
            decision,
            matched_rule_identity,
        } => {
            assert_eq!(request_id, "ask-0000000000000001");
            assert_eq!(decision.as_str(), "allow_always");
            assert_eq!(matched_rule_identity, "nested:nu.command:*");
        }
        other => panic!("unexpected variant: {other:?}"),
    }

    let timed_out = PermissionEvent::DecisionTimedOut {
        request_id: "ask-0000000000000002".to_string(),
    };
    match timed_out {
        PermissionEvent::DecisionTimedOut { request_id } => {
            assert_eq!(request_id, "ask-0000000000000002");
        }
        other => panic!("unexpected variant: {other:?}"),
    }

    let ignored = PermissionEvent::DecisionIgnored {
        request_id: "ask-0000000000000003".to_string(),
        reason: "rule_identity_mismatch".to_string(),
    };
    match ignored {
        PermissionEvent::DecisionIgnored { request_id, reason } => {
            assert_eq!(request_id, "ask-0000000000000003");
            assert_eq!(reason, "rule_identity_mismatch");
        }
        other => panic!("unexpected variant: {other:?}"),
    }
}
