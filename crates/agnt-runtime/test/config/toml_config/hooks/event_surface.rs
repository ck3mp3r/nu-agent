//! The `HookEvent` surface: names, classes, and serde round trips.

use super::*;

#[test]
fn hook_event_names_and_classes_cover_the_v1_surface() -> Result<()> {
    // -- Setup & Fixtures
    let expected = [
        (
            HookEvent::TurnStarted,
            "turn_started",
            HookEventClass::Session,
        ),
        (
            HookEvent::TurnCompleted,
            "turn_completed",
            HookEventClass::Session,
        ),
        (
            HookEvent::ToolStarted,
            "tool_started",
            HookEventClass::Session,
        ),
        (
            HookEvent::ToolCompleted,
            "tool_completed",
            HookEventClass::Session,
        ),
        (
            HookEvent::SessionStarted,
            "session_started",
            HookEventClass::Session,
        ),
        (
            HookEvent::SessionEnded,
            "session_ended",
            HookEventClass::Session,
        ),
        (
            HookEvent::PermissionRequested,
            "permission_requested",
            HookEventClass::Session,
        ),
        (
            HookEvent::PermissionDecisionTimedOut,
            "permission_decision_timed_out",
            HookEventClass::Session,
        ),
        (
            HookEvent::PermissionDecisionSubmitted,
            "permission_decision_submitted",
            HookEventClass::Session,
        ),
        (
            HookEvent::PermissionDecisionIgnored,
            "permission_decision_ignored",
            HookEventClass::Session,
        ),
        (
            HookEvent::PermissionRequestCancelled,
            "permission_request_cancelled",
            HookEventClass::Session,
        ),
        (
            HookEvent::PermissionAutoResolved,
            "permission_auto_resolved",
            HookEventClass::Session,
        ),
        (
            HookEvent::CompactionStarted,
            "compaction_started",
            HookEventClass::Session,
        ),
        (
            HookEvent::CompactionCompleted,
            "compaction_completed",
            HookEventClass::Session,
        ),
        (
            HookEvent::CompactionFailed,
            "compaction_failed",
            HookEventClass::Session,
        ),
        (
            HookEvent::AgentSwitched,
            "agent_switched",
            HookEventClass::Session,
        ),
        (
            HookEvent::ModelSwitched,
            "model_switched",
            HookEventClass::Session,
        ),
        (
            HookEvent::AgentStarted,
            "agent_started",
            HookEventClass::Agent,
        ),
        (
            HookEvent::AgentShutdown,
            "agent_shutdown",
            HookEventClass::Agent,
        ),
    ];

    // -- Exec & Check
    for (event, name, class) in expected {
        assert_eq!(event.name(), name, "the event name must be {name}");
        assert_eq!(
            event.class(),
            class,
            "{name} must be in the {class:?} class"
        );
        assert_eq!(
            HookEvent::from_name(name),
            Some(event),
            "{name} must parse back to its variant"
        );
    }
    Ok(())
}

#[test]
fn hook_event_serde_round_trips_the_v1_names() -> Result<()> {
    // -- Setup & Fixtures
    let names = [
        "turn_started",
        "turn_completed",
        "tool_started",
        "tool_completed",
        "session_started",
        "session_ended",
        "permission_requested",
        "permission_decision_timed_out",
        "permission_decision_submitted",
        "permission_decision_ignored",
        "permission_request_cancelled",
        "permission_auto_resolved",
        "compaction_started",
        "compaction_completed",
        "compaction_failed",
        "agent_switched",
        "model_switched",
        "agent_started",
        "agent_shutdown",
    ];

    // -- Exec & Check
    assert_eq!(
        HookEvent::NAMES,
        names,
        "NAMES must list every v1 event name in declaration order"
    );
    for name in names {
        let event = HookEvent::from_name(name).ok_or(format!("{name} must be a known event"))?;
        let encoded = serde_json::to_string(&event)?;
        assert_eq!(encoded, format!("\"{name}\""));
        let decoded: HookEvent = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, event);
    }
    Ok(())
}
