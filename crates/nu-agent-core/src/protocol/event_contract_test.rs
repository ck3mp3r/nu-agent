use crate::protocol::event::UiEvent;

#[test]
fn ui_event_contract_exposes_required_variants() {
    let events = [
        UiEvent::LlmStart,
        UiEvent::Tick,
        UiEvent::LlmEnd {
            response_chars: 12,
            tool_calls: 1,
            input_tokens: 7,
            output_tokens: 5,
            total_tokens: 12,
        },
        UiEvent::ToolStart {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
        },
        UiEvent::ToolEnd {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: "[]".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
        UiEvent::PermissionRequested {
            request_id: "ask-0000000000000001".to_string(),
            context: crate::protocol::event::PermissionRequestContext {
                tool: "nu__run".to_string(),
                source: "closure".to_string(),
                mode: Some("apply".to_string()),
                matched_rule_identity: "nested:nu__run.command:*".to_string(),
                scope: "nested".to_string(),
                target_field: Some("command".to_string()),
                pattern: "*".to_string(),
                summary: "tool[nu__run] args={\"command\":\"echo hi\"}".to_string(),
                pre_authorize_display: None,
            },
        },
        UiEvent::PermissionDecisionSubmitted {
            request_id: "ask-0000000000000001".to_string(),
            decision: crate::protocol::event::PermissionDecision::AllowOnce,
            matched_rule_identity: "nested:nu__run.command:*".to_string(),
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
        UiEvent::CompactionTriggered {
            source: "auto_threshold".to_string(),
            summarized_count: 3,
            kept_recent_count: 2,
            summary_preview: "summary preview".to_string(),
            summary_body: "summary body".to_string(),
        },
        UiEvent::CompactionFailed {
            source: "auto_threshold".to_string(),
            message: "failed".to_string(),
        },
        UiEvent::AssistantMessage {
            text: "done".to_string(),
        },
        UiEvent::Completed { tool_calls: 1 },
    ];

    assert_eq!(events.len(), 16);
}

#[test]
fn permission_event_field_shape_is_explicit_and_stable() {
    let requested = UiEvent::PermissionRequested {
        request_id: "ask-0000000000000001".to_string(),
        context: crate::protocol::event::PermissionRequestContext {
            tool: "nu__run(command=echo hi)".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            matched_rule_identity: "nested:nu__run.command:*".to_string(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "tool[nu__run] args={\"command\":\"echo hi\"}".to_string(),
            pre_authorize_display: None,
        },
    };
    match requested {
        UiEvent::PermissionRequested {
            request_id,
            context,
        } => {
            assert_eq!(request_id, "ask-0000000000000001");
            assert_eq!(context.tool, "nu__run(command=echo hi)");
            assert_eq!(context.source, "closure");
            assert_eq!(context.mode.as_deref(), Some("apply"));
            assert_eq!(context.matched_rule_identity, "nested:nu__run.command:*");
            assert_eq!(context.scope, "nested");
            assert_eq!(context.target_field.as_deref(), Some("command"));
            assert_eq!(context.pattern, "*");
            assert!(context.summary.starts_with("tool[nu__run] args="));
            assert!(context.pre_authorize_display.is_none());
        }
        other => panic!("unexpected variant: {other:?}"),
    }

    let submitted = UiEvent::PermissionDecisionSubmitted {
        request_id: "ask-0000000000000001".to_string(),
        decision: crate::protocol::event::PermissionDecision::AllowAlways,
        matched_rule_identity: "nested:nu__run.command:*".to_string(),
    };
    match submitted {
        UiEvent::PermissionDecisionSubmitted {
            request_id,
            decision,
            matched_rule_identity,
        } => {
            assert_eq!(request_id, "ask-0000000000000001");
            assert_eq!(decision.as_str(), "allow_always");
            assert_eq!(matched_rule_identity, "nested:nu__run.command:*");
        }
        other => panic!("unexpected variant: {other:?}"),
    }

    let timed_out = UiEvent::PermissionDecisionTimedOut {
        request_id: "ask-0000000000000002".to_string(),
    };
    match timed_out {
        UiEvent::PermissionDecisionTimedOut { request_id } => {
            assert_eq!(request_id, "ask-0000000000000002");
        }
        other => panic!("unexpected variant: {other:?}"),
    }

    let ignored = UiEvent::PermissionDecisionIgnored {
        request_id: "ask-0000000000000003".to_string(),
        reason: "rule_identity_mismatch".to_string(),
    };
    match ignored {
        UiEvent::PermissionDecisionIgnored { request_id, reason } => {
            assert_eq!(request_id, "ask-0000000000000003");
            assert_eq!(reason, "rule_identity_mismatch");
        }
        other => panic!("unexpected variant: {other:?}"),
    }
}
