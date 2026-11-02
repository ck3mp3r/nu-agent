//! Tool narration contracts: the call window bracket and its dispositions.

use crate::bus::Bus;
use crate::records::{SessionRecord, ToolCall, ToolOutcome, ToolRecord, ToolStatus, TurnRecord};

use crate::test_support::Result;

/// The tool windows of a turn: each dispatched call narrates `Started` then
/// exactly one `Completed`, in stream order, correlated by the derived call
/// id — the anchor consumers key on, never rendered text.
#[tokio::test]
async fn test_records_tool_windows_narrate_started_then_completed_per_call() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();
    let first = ToolCall::new("nu__shell", "builtin", r#"{"command":"ls"}"#);
    let second = ToolCall::new("fs__read", "builtin", r#"{"path":"a.txt"}"#);
    assert_ne!(first.id, second.id, "distinct calls must key distinctly");

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: first.clone(),
        }))
        .map_err(|e| format!("publish tool started a: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Completed {
            call: first.clone(),
            outcome: ToolOutcome {
                status: ToolStatus::Success,
                result: "a.txt".to_string(),
                message: None,
            },
        }))
        .map_err(|e| format!("publish tool completed a: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: second.clone(),
        }))
        .map_err(|e| format!("publish tool started b: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Completed {
            call: second.clone(),
            outcome: ToolOutcome {
                status: ToolStatus::Failed,
                result: String::new(),
                message: Some("no such file".to_string()),
            },
        }))
        .map_err(|e| format!("publish tool completed b: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            tool_calls: 2,
            duration_ms: 9,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut completed: Vec<(crate::records::ToolCallId, ToolStatus)> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Tool(ToolRecord::Started { .. }) => tags.push("tool_started"),
                SessionRecord::Tool(ToolRecord::Completed { call, outcome }) => {
                    tags.push("tool_completed");
                    completed.push((call.id.clone(), outcome.status));
                }
                SessionRecord::Inference(_) => tags.push("inference"),
                SessionRecord::Turn(TurnRecord::Completed { .. }) => tags.push("turn"),
                SessionRecord::Permission(_) => tags.push("permission"),
                SessionRecord::Notice(_) => tags.push("notice"),
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
            "tool_completed",
            "tool_started",
            "tool_completed",
            "turn"
        ],
        "each call brackets as started then completed, in stream order"
    );
    assert_eq!(
        completed,
        vec![
            (first.id.clone(), ToolStatus::Success),
            (second.id.clone(), ToolStatus::Failed)
        ],
        "the completed records carry the call id and the enumerated disposition"
    );
    Ok(())
}

/// A permission-denied call still narrates the pair and settles as the
/// enumerated `Cancelled` disposition; the denial reason is content, not a
/// status.
#[tokio::test]
async fn test_records_denied_call_settles_cancelled_without_running() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();
    let call = ToolCall::new("nu__shell", "builtin", r#"{"command":"rm -rf /"}"#);

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: call.clone(),
        }))
        .map_err(|e| format!("publish tool started: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Tool(ToolRecord::Completed {
            call: call.clone(),
            outcome: ToolOutcome {
                status: ToolStatus::Cancelled,
                result: String::new(),
                message: Some("denied by policy".to_string()),
            },
        }))
        .map_err(|e| format!("publish tool completed: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            tool_calls: 0,
            duration_ms: 3,
        }))
        .map_err(|e| format!("publish turn: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut denied: Vec<(ToolStatus, Option<String>)> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Tool(ToolRecord::Started { .. }) => tags.push("tool_started"),
                SessionRecord::Tool(ToolRecord::Completed { outcome, .. }) => {
                    tags.push("tool_completed");
                    denied.push((outcome.status, outcome.message.clone()));
                }
                SessionRecord::Inference(_) => tags.push("inference"),
                SessionRecord::Turn(TurnRecord::Completed { .. }) => tags.push("turn"),
                SessionRecord::Permission(_) => tags.push("permission"),
                SessionRecord::Notice(_) => tags.push("notice"),
            },
            Err(crate::bus::ReceiveError::Closed) => break,
            Err(crate::bus::ReceiveError::Lagged(_)) => {
                panic!("under capacity, lag is a contract violation")
            }
        }
    }
    assert_eq!(
        tags,
        ["tool_started", "tool_completed", "turn"],
        "a denied call still narrates its full pair before the turn closes"
    );
    assert_eq!(
        denied,
        vec![(ToolStatus::Cancelled, Some("denied by policy".to_string()))],
        "denial settles as the enumerated disposition; the reason is content"
    );
    Ok(())
}
