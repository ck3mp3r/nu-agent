use super::*;

#[tokio::test]
async fn loop_hydrate_replays_the_prior_session_without_the_select_loop() -> TResult {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let messages = vec![
        UiMessageSnapshot::new("user", "from history"),
        UiMessageSnapshot::new("assistant", "from reply"),
    ];
    let mut stream_rx = bus.stream().subscribe();
    let mut lifecycle_rx = bus.lifecycle().subscribe();
    let mut loop_ = Loop::compose(
        runtime,
        LoopConfig::default()
            .with_hydration(messages.clone(), Some(321))
            .with_bus(bus),
    );

    // -- Exec
    loop_.hydrate().await;

    // -- Check
    // The replay is a record log: the hydrated marker opens it, then the
    // projected rows follow.
    let marker = stream_rx
        .recv()
        .await
        .map_err(|e| format!("stream record: {e:?}"))?;
    assert!(
        matches!(
            marker,
            SessionRecord::Session(SessionLifecycle::Started { hydrated: true, .. })
        ),
        "hydrate must open the replay with the hydrated marker, got {marker:?}"
    );
    let replayed = stream_rx
        .recv()
        .await
        .map_err(|e| format!("stream record: {e:?}"))?;
    assert!(
        matches!(
            replayed,
            SessionRecord::Prompt(ref prompt) if prompt.text == "from history"
        ),
        "the replay must carry the stored prompt, got {replayed:?}"
    );

    let started = lifecycle_rx
        .recv()
        .await
        .map_err(|e| format!("session event: {e:?}"))?;
    assert!(
        matches!(
            started,
            crate::bus::LifecycleEvent::SessionStarted { hydrated: true, .. }
        ),
        "hydrate must announce a hydrated session start"
    );
    Ok(())
}
