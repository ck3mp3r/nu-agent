//! Authorization narration contracts: the ask-path bracket, the policy path,
//! and the notice surface.

use crate::bus::Bus;
use crate::records::{
    AutoResolution, NoticeKind, NoticeRecord, PermissionDecision, PermissionRecord, RequestId,
    SessionRecord, ToolCall, ToolOutcome, ToolRecord, ToolStatus, TurnRecord,
};

use crate::test_support::Result;

/// The authorization bracket of an ask-path denial: the call starts, the card
/// opens, the decision arrives as an enum, the call settles as not-run, and
/// the turn closes — in exactly that order.
#[tokio::test]
async fn test_records_ask_path_denial_brackets_call_and_permission() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();
    let call = ToolCall::new(
        "nu__shell",
        crate::ToolSource::Builtin,
        r#"{"command":"rm -rf /"}"#,
    );
    let request_id = RequestId::new("req-1");

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: call.clone(),
        }))
        .map_err(|e| format!("publish tool started: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Permission(PermissionRecord::Requested {
            request_id: request_id.clone(),
            call_id: call.id.clone(),
            tool: "nu__shell".to_string(),
            source: "builtin".to_string(),
            rule_identity: "deny_rm".to_string(),
            scope: "global".to_string(),
            pattern: "rm -rf".to_string(),
            target_field: None,
            mode: None,
            summary: "Run rm -rf /".to_string(),
        }))
        .map_err(|e| format!("publish permission requested: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Permission(PermissionRecord::Decided {
            request_id: request_id.clone(),
            decision: PermissionDecision::Deny,
            rule_identity: "deny_rm".to_string(),
        }))
        .map_err(|e| format!("publish permission decided: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Completed {
            call: call.clone(),
            outcome: ToolOutcome {
                status: ToolStatus::Cancelled,
                result: String::new(),
                message: Some("denied by policy".to_string()),
                kind: Some(crate::records::ToolErrorKind::Denied),
            },
        }))
        .map_err(|e| format!("publish tool completed: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: 2,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut decisions: Vec<PermissionDecision> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Tool(ToolRecord::Started { .. }) => tags.push("tool_started"),
                SessionRecord::Tool(ToolRecord::Completed { .. }) => tags.push("tool_completed"),
                SessionRecord::Permission(PermissionRecord::Requested { .. }) => {
                    tags.push("permission_requested")
                }
                SessionRecord::Permission(PermissionRecord::Decided { decision, .. }) => {
                    tags.push("permission_decided");
                    decisions.push(*decision);
                }
                SessionRecord::Permission(_) => tags.push("permission_other"),
                SessionRecord::Notice(_) => tags.push("notice"),
                SessionRecord::Compaction(_) => tags.push("compaction"),
                SessionRecord::Session(_) => tags.push("session"),
                SessionRecord::Inference(_) => tags.push("inference"),
                SessionRecord::Turn(TurnRecord::Completed { .. }) => tags.push("turn"),
            },
            Err(crate::bus::ReceiveError::Closed) => break,
            Err(crate::bus::ReceiveError::Lagged(_)) => {
                panic!("under capacity, lag is a contract violation")
            }
        }
    }
    assert_eq!(
        tags,
        [
            "tool_started",
            "permission_requested",
            "permission_decided",
            "tool_completed",
            "turn"
        ],
        "the authorization bracket orders the call, the card, the decision, the settle and the close"
    );
    assert_eq!(
        decisions,
        [PermissionDecision::Deny],
        "the decision is an enumerated answer, never text"
    );
    Ok(())
}

/// The policy path: a call resolved without prompting narrates `AutoResolved`
/// and no `Requested` — the bracket shrinks to policy action + settle.
#[tokio::test]
async fn test_records_policy_path_narrates_auto_resolution_without_request() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();
    let call = ToolCall::new(
        "nu__shell",
        crate::ToolSource::Builtin,
        r#"{"command":"ls"}"#,
    );

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: call.clone(),
        }))
        .map_err(|e| format!("publish tool started: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Permission(PermissionRecord::AutoResolved {
            rule_identity: "allow_ls".to_string(),
            action: AutoResolution::Allow,
            reason: Some("matched allow rule".to_string()),
        }))
        .map_err(|e| format!("publish auto resolved: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Completed {
            call: call.clone(),
            outcome: ToolOutcome {
                status: ToolStatus::Success,
                result: "a.txt".to_string(),
                message: None,
                kind: None,
            },
        }))
        .map_err(|e| format!("publish tool completed: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: 6,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Tool(ToolRecord::Started { .. }) => tags.push("tool_started"),
                SessionRecord::Tool(ToolRecord::Completed { .. }) => tags.push("tool_completed"),
                SessionRecord::Permission(PermissionRecord::AutoResolved { .. }) => {
                    tags.push("permission_auto_resolved")
                }
                SessionRecord::Permission(_) => tags.push("permission_other"),
                SessionRecord::Notice(_) => tags.push("notice"),
                SessionRecord::Compaction(_) => tags.push("compaction"),
                SessionRecord::Session(_) => tags.push("session"),
                SessionRecord::Inference(_) => tags.push("inference"),
                SessionRecord::Turn(TurnRecord::Completed { .. }) => tags.push("turn"),
            },
            Err(crate::bus::ReceiveError::Closed) => break,
            Err(crate::bus::ReceiveError::Lagged(_)) => {
                panic!("under capacity, lag is a contract violation")
            }
        }
    }
    assert_eq!(
        tags,
        [
            "tool_started",
            "permission_auto_resolved",
            "tool_completed",
            "turn"
        ],
        "a policy-resolved call never narrates a request card"
    );
    Ok(())
}

/// A notice narrates an enumerated kind plus content text: the kind decides
/// how it is treated, the text is content — never sniffed.
#[tokio::test]
async fn test_records_notice_carries_enumerated_kind_and_content() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Notice(NoticeRecord {
            kind: NoticeKind::Warning,
            text: "conversation exceeds the context window".to_string(),
        }))
        .map_err(|e| format!("publish warning: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Notice(NoticeRecord {
            kind: NoticeKind::TurnError,
            text: "provider returned 500".to_string(),
        }))
        .map_err(|e| format!("publish turn error: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: 5,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut notices: Vec<NoticeRecord> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(SessionRecord::Notice(notice)) => notices.push(notice),
            Ok(_) => {}
            Err(crate::bus::ReceiveError::Closed) => break,
            Err(crate::bus::ReceiveError::Lagged(_)) => {
                panic!("under capacity, lag is a contract violation")
            }
        }
    }
    assert_eq!(
        notices
            .iter()
            .map(|notice| notice.kind)
            .collect::<Vec<NoticeKind>>(),
        [NoticeKind::Warning, NoticeKind::TurnError],
        "notice kinds are an enumerated producer fact"
    );
    assert_eq!(
        notices
            .iter()
            .map(|notice| notice.text.as_str())
            .collect::<Vec<&str>>(),
        [
            "conversation exceeds the context window",
            "provider returned 500"
        ],
        "notice text is content"
    );
    Ok(())
}
