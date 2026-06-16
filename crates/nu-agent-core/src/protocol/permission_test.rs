use std::time::Duration;

use crate::protocol::{
    event::{PermissionDecision, PermissionRequestContext, UiEvent},
    permission::{PermissionController, PermissionRequest, PermissionResolution, RequestError},
};

fn request_with_id(request_id: &str) -> PermissionRequest {
    PermissionRequest {
        request_id: request_id.to_string(),
        context: PermissionRequestContext {
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
    }
}

#[test]
fn begin_request_rejects_duplicate_request_id() {
    let controller = PermissionController::new(Duration::from_millis(100));
    let first = controller.begin_request(request_with_id("ask-0000000000000001"));
    assert!(first.is_ok());

    let second = controller.begin_request(request_with_id("ask-0000000000000001"));
    assert!(matches!(second, Err(RequestError::AlreadyWaiting)));
}

#[test]
fn await_resolution_emits_ignored_event_for_rule_identity_mismatch_then_times_out() {
    let controller = PermissionController::new(Duration::from_millis(30));
    let (token, _event) = controller
        .begin_request(request_with_id("ask-0000000000000002"))
        .expect("begin request");

    let outcome = token.submit(crate::protocol::event::PermissionDecisionSubmission {
        request_id: token.request_id().to_string(),
        decision: PermissionDecision::AllowAlways,
        matched_rule_identity: "wrong-rule".to_string(),
    });
    assert_eq!(
        outcome,
        crate::protocol::permission::SubmitOutcome::Ignored {
            reason: "rule_identity_mismatch"
        }
    );

    let (resolution, events) = controller.await_resolution(&token);
    assert_eq!(resolution, PermissionResolution::TimedOut);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, UiEvent::PermissionDecisionTimedOut { .. }))
    );
}

#[test]
fn await_resolution_ignores_stale_submission_and_accepts_matching_submission() {
    let controller = PermissionController::new(Duration::from_secs(1));
    let (token, _event) = controller
        .begin_request(request_with_id("ask-0000000000000003"))
        .expect("begin request");

    let sender = token.sender_clone();
    sender
        .send(crate::protocol::event::PermissionDecisionSubmission {
            request_id: "stale-request-id".to_string(),
            decision: PermissionDecision::AllowAlways,
            matched_rule_identity: token.matched_rule_identity().to_string(),
        })
        .expect("send stale submission");
    sender
        .send(crate::protocol::event::PermissionDecisionSubmission {
            request_id: token.request_id().to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: token.matched_rule_identity().to_string(),
        })
        .expect("send matching submission");

    let (resolution, events) = controller.await_resolution(&token);
    assert_eq!(
        resolution,
        PermissionResolution::Decision {
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: token.matched_rule_identity().to_string(),
        }
    );
    assert!(events.iter().any(|event| matches!(
        event,
        UiEvent::PermissionDecisionIgnored {
            request_id,
            reason
        } if request_id == "stale-request-id" && reason == "stale_or_unknown_request"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        UiEvent::PermissionDecisionSubmitted { request_id, decision, .. }
            if request_id == token.request_id() && *decision == PermissionDecision::AllowOnce
    )));
}
