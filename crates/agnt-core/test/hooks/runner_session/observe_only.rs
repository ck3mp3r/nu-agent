//! The observe-only guarantee: the runner publishes nothing.

use super::*;

#[tokio::test]
async fn test_hooks_runner_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook(
        "audit",
        &[HookEvent::TurnStarted, HookEvent::SessionStarted],
    )];
    let mut compaction_rx = bus.compaction().subscribe();
    let mut cancel_rx = bus.cancel().subscribe();
    let mut external_rx = bus.external().subscribe();
    let mut ui_state_rx = bus.ui_state().subscribe();
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .turn()
        .send(TurnEvent::Started {
            prompt: "hello".to_string(),
            task_id: None,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert_eq!(process.calls().len(), 1, "the hook must have run");
    assert!(
        compaction_rx.try_recv().is_err(),
        "the runner must not publish a compaction event"
    );
    assert!(
        cancel_rx.try_recv().is_err(),
        "the runner must not publish a cancel event"
    );
    assert!(
        external_rx.try_recv().is_err(),
        "the runner must not publish an external event"
    );
    assert!(
        ui_state_rx.try_recv().is_err(),
        "the runner must not publish a ui-state event"
    );
    Ok(())
}
