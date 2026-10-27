//! The `model_switched` event: payload shape, subscription, and the tracked
//! session id.
//!
//! The mirror of `agent_switched`: session-class, riding `bus.session()`, with
//! the envelope stamped from the session's hook context. Unlike a persona
//! switch, a model switch always has a previous identity to report, so `from`
//! is a plain string.

use super::*;

#[tokio::test]
async fn test_hooks_runner_model_switched_invokes_with_from_and_to() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::ModelSwitched])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::ModelSwitched {
            from: "ollama/llama3".to_string(),
            to: "openai/gpt-4o".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a hook that declares model_switched must run on the session channel; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "model_switched");
    let body = payload(&calls[0])?;
    assert_eq!(body["from"], "ollama/llama3");
    assert_eq!(body["to"], "openai/gpt-4o");
    assert_eq!(
        body.as_object().map(|object| object.len()),
        Some(2),
        "the payload carries the two identities only: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_model_switched_skips_a_hook_that_does_not_declare_it() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::SessionStarted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::ModelSwitched {
            from: "ollama/llama3".to_string(),
            to: "openai/gpt-4o".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "a hook that does not declare model_switched must not run on it"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_model_switched_leaves_the_tracked_session_id_alone() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::ModelSwitched])];
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
        .send(SessionEvent::ModelSwitched {
            from: "ollama/llama3".to_string(),
            to: "openai/gpt-4o".to_string(),
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
        envelope["model"], "openai/gpt-4o",
        "the envelope carries the model the context holds"
    );
    assert_eq!(
        envelope["agent"], "reviewer",
        "a model switch leaves the persona the context holds"
    );
    Ok(())
}
