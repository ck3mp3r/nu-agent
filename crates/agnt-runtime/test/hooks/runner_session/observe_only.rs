//! The observe-only guarantee: the runner publishes nothing.

use super::*;

#[tokio::test]
async fn test_hooks_runner_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook(
        "audit",
        &[HookEvent::TurnStarted, HookEvent::SessionStarted],
    )];
    // The runner reads the stream and the lifecycle; every other concern is an
    // output it must never write.
    let mut control_rx = bus.control().subscribe();
    let mut cancel_rx = bus.cancel().subscribe();
    let mut narration_rx = bus.narration().subscribe();
    let mut facts_rx = bus.facts().subscribe();
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.lifecycle().publish(LifecycleEvent::TurnStarted {
        prompt: "hello".to_string(),
        task_id: None,
    });
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert_eq!(process.calls().len(), 1, "the hook must have run");
    assert!(
        control_rx.try_recv().is_err(),
        "the runner must not publish a control command"
    );
    assert!(
        cancel_rx.try_recv().is_err(),
        "the runner must not publish a cancel event"
    );
    assert!(
        narration_rx.try_recv().is_err(),
        "the runner must not publish narration"
    );
    assert!(
        facts_rx.try_recv().is_err(),
        "the runner must not publish a fact"
    );
    Ok(())
}
