//! The `agent_switched` event: payload shape, subscription, and the tracked
//! session id.
//!
//! The event is session-class, so it rides `bus.session()` like every other
//! session event: the runner subscribes that channel unconditionally, and the
//! envelope is stamped from the session's hook context — which is what makes a
//! switch's new agent and model visible to the handler.

use super::*;

#[tokio::test]
async fn test_hooks_runner_agent_switched_invokes_with_from_and_to() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::AgentSwitched])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::AgentSwitched {
            from: Some("coder".to_string()),
            to: "reviewer".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a hook that declares agent_switched must run on the session channel; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "agent_switched");
    let body = payload(&calls[0])?;
    assert_eq!(body["from"], "coder");
    assert_eq!(body["to"], "reviewer");
    assert_eq!(
        body.as_object().map(|object| object.len()),
        Some(2),
        "the payload carries the switch's two slugs only: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_agent_switched_carries_a_null_from_when_none_was_loaded() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::AgentSwitched])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::AgentSwitched {
            from: None,
            to: "reviewer".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a first switch must still invoke; got: {calls:?}"
    );
    let body = payload(&calls[0])?;
    assert_eq!(
        body["from"],
        serde_json::Value::Null,
        "a session with no persona yet reports a null from: {body}"
    );
    assert_eq!(body["to"], "reviewer");
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_agent_switched_skips_a_hook_that_does_not_declare_it() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::SessionStarted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::AgentSwitched {
            from: Some("coder".to_string()),
            to: "reviewer".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "a hook that does not declare agent_switched must not run on it"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_agent_switched_leaves_the_tracked_session_id_alone() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::AgentSwitched])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::Started {
            session_id: "sess-1".to_string(),
            hydrated: false,
        })
        .await;
    let _ = bus
        .session()
        .send(SessionEvent::AgentSwitched {
            from: None,
            to: "reviewer".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "only the declared event invokes; got: {calls:?}"
    );
    let envelope = envelope_json(&calls[0])?;
    assert_eq!(
        envelope["session_id"], "sess-1",
        "the switch must not clear the id Started tracked"
    );
    assert_eq!(
        envelope["agent"], "reviewer",
        "the envelope carries the persona the context holds"
    );
    assert_eq!(
        envelope["model"], "openai/gpt-4o",
        "the envelope carries the model the context holds"
    );
    Ok(())
}
