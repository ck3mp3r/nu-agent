//! Session lifecycle narration contracts: identity, switches, and the close.

use crate::bus::Bus;
use crate::records::{SessionId, SessionLifecycle, SessionRecord};

use crate::test_support::Result;

/// The session lifecycle order: it opens (with its hydration state), every
/// identity switch narrates as data, and it closes exactly once, last.
#[tokio::test]
async fn test_records_session_lifecycle_orders_start_switches_and_close() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.stream().subscribe();
    let first = SessionId::new("s-1");
    let second = SessionId::new("s-2");

    // -- Exec
    bus.stream()
        .publish(SessionRecord::Session(SessionLifecycle::Started {
            session_id: first.clone(),
            hydrated: true,
        }))
        .map_err(|e| format!("publish started: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Session(SessionLifecycle::AgentSwitched {
            from: None,
            to: "maker".to_string(),
        }))
        .map_err(|e| format!("publish agent switched: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Session(SessionLifecycle::ModelSwitched {
            from: "openai/gpt-5".to_string(),
            to: "anthropic/claude".to_string(),
        }))
        .map_err(|e| format!("publish model switched: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Session(SessionLifecycle::Switched {
            from: Some(first.clone()),
            to: second.clone(),
        }))
        .map_err(|e| format!("publish switched: {e:?}"))?;
    bus.stream()
        .publish(SessionRecord::Session(SessionLifecycle::Ended {
            session_id: second.clone(),
        }))
        .map_err(|e| format!("publish ended: {e:?}"))?;
    drop(bus);

    // -- Check
    let mut tags: Vec<&str> = Vec::new();
    let mut hydrated: Option<bool> = None;
    let mut ended: Option<SessionId> = None;
    let mut model_switches: Vec<(String, String)> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(record) => match &record {
                SessionRecord::Session(lifecycle) => match lifecycle {
                    SessionLifecycle::Started {
                        hydrated: hydration,
                        ..
                    } => {
                        tags.push("session_started");
                        hydrated = Some(*hydration);
                    }
                    SessionLifecycle::AgentSwitched { .. } => tags.push("session_agent_switched"),
                    SessionLifecycle::ModelSwitched { from, to } => {
                        tags.push("session_model_switched");
                        model_switches.push((from.clone(), to.clone()));
                    }
                    SessionLifecycle::Switched { .. } => tags.push("session_switched"),
                    SessionLifecycle::Ended { session_id } => {
                        tags.push("session_ended");
                        ended = Some(session_id.clone());
                    }
                },
                SessionRecord::Inference(_) => tags.push("inference"),
                SessionRecord::Tool(_) => tags.push("tool"),
                SessionRecord::Permission(_) => tags.push("permission"),
                SessionRecord::Compaction(_) => tags.push("compaction"),
                SessionRecord::Prompt(_) => tags.push("prompt"),
                SessionRecord::Notice(_) => tags.push("notice"),
                SessionRecord::Turn(_) => tags.push("turn"),
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
            "session_started",
            "session_agent_switched",
            "session_model_switched",
            "session_switched",
            "session_ended"
        ],
        "the session lifecycle narrates in order"
    );
    assert_eq!(hydrated, Some(true), "hydration state is data");
    assert_eq!(
        model_switches,
        [("openai/gpt-5".to_string(), "anthropic/claude".to_string())],
        "a switch carries both identities as data"
    );
    assert_eq!(
        ended,
        Some(second),
        "the session closes under the id it ended on"
    );
    Ok(())
}
