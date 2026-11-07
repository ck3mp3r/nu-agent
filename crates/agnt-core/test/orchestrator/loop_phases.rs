use super::*;

use agnt_runtime::bus::SessionEvent;

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
    let mut ui_state_rx = bus.ui_state().subscribe();
    let mut session_rx = bus.session().subscribe();
    let mut loop_ = Loop::compose(
        runtime,
        LoopConfig::default()
            .with_hydration(messages.clone(), Some(321))
            .with_bus(bus),
    );

    // -- Exec
    loop_.hydrate().await;

    // -- Check
    let hydrated = ui_state_rx
        .recv()
        .await
        .map_err(|e| format!("ui state event: {e:?}"))?;
    match hydrated {
        UiStateEvent::HydrateTranscript {
            messages: replayed,
            last_total_tokens,
        } => {
            assert_eq!(replayed, messages);
            assert_eq!(last_total_tokens, Some(321));
        }
        _ => return Err("expected HydrateTranscript on the ui-state channel".into()),
    }

    let started = session_rx
        .recv()
        .await
        .map_err(|e| format!("session event: {e:?}"))?;
    assert!(
        matches!(started, SessionEvent::Started { hydrated: true, .. }),
        "hydrate must announce a hydrated session start"
    );
    Ok(())
}
