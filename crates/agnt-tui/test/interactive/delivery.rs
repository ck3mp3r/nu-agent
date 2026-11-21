//! Eventual delivery: a full channel keeps an effect pending and the loop live,
//! a closed one ends the loop, and a blocked quit gives up on a deadline.

use super::*;
use crate::interactive::RENDER_TICK;

/// WHEN the event channel is full, THE loop SHALL keep the prompt pending and
/// SHALL keep folding input, and THE pending prompt SHALL land on a later flush.
#[tokio::test]
async fn test_render_loop_keeps_a_prompt_pending_while_the_channel_is_full() -> Result<()> {
    // -- Setup & Fixtures: one slot, already taken, so the next send is full.
    let bus = Bus::default();
    let bus_for_loop = bus.clone();
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let cancel_controller = CancelController::default();
    let (event_tx, mut event_rx) = mpsc::channel::<OrchestratorEvent>(1);
    let (terminal_tx, terminal_rx) = mpsc::channel::<TerminalInput>(8);
    let (_branch_tx, branch_rx) = mpsc::channel::<()>(8);
    let mut live: Option<&mut ratatui::Terminal<ratatui::backend::TestBackend>> = None;

    event_tx
        .send(OrchestratorEvent::CancelRequested)
        .await
        .map_err(|_| "should fill the event channel")?;

    let loop_handle = tokio::spawn(async move {
        run_render_loop(
            &mut coordinator,
            &cancel_controller,
            event_tx,
            terminal_rx,
            &mut live,
            crate::interactive::RenderLoopChannels {
                branch_rx,
                theme_persist_tx: None,
                stream: bus_for_loop.stream(),
                narration: bus_for_loop.narration(),
                control: bus_for_loop.control(),
                lifecycle: bus_for_loop.lifecycle(),
                facts: bus_for_loop.facts(),
            },
        )
        .await;
        coordinator
    });
    tokio::task::yield_now().await;

    // -- Exec: submit a prompt while the channel cannot take it.
    for key in [
        TerminalKey::Char('h'),
        TerminalKey::Char('i'),
        TerminalKey::Enter,
    ] {
        terminal_tx
            .send(stamped(TerminalEvent::Key(key)))
            .await
            .map_err(|_| "should send the submit keys")?;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    // -- Check: a later key still folds, so the loop never blocked.
    terminal_tx
        .send(stamped(TerminalEvent::Key(TerminalKey::Char('x'))))
        .await
        .map_err(|_| "should send the trailing key")?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // -- Exec: free the slot, then fold one more key so the flush retries.
    let blocked = event_rx
        .recv()
        .await
        .ok_or("the pre-filled event must arrive")?;
    assert!(
        matches!(blocked, OrchestratorEvent::CancelRequested),
        "the pre-filled event must be the one that arrived"
    );
    terminal_tx
        .send(stamped(TerminalEvent::Key(TerminalKey::Char('y'))))
        .await
        .map_err(|_| "should send the retry key")?;
    let delivered = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
        .await
        .map_err(|_| "the pending prompt must land after the slot frees")?
        .ok_or("the pending prompt must arrive")?;

    // -- Check: the pending prompt landed, and both later keys folded.
    assert!(
        matches!(&delivered, OrchestratorEvent::PromptSubmitted { text } if text == "hi"),
        "the pending prompt must arrive unchanged"
    );
    drop(terminal_tx);
    let coordinator = tokio::time::timeout(Duration::from_secs(1), loop_handle)
        .await
        .map_err(|_| "the loop must exit when the terminal channel closes")?
        .map_err(|err| format!("the loop task must not panic: {err}"))?;
    let draft = coordinator.textarea.lines().join("\n");
    assert!(
        draft.contains('x') && draft.contains('y'),
        "both keys folded while the consumer was busy, draft: {draft:?}"
    );
    Ok(())
}

/// WHEN the event channel is closed, THE loop SHALL stop: the consumer is gone.
#[tokio::test]
async fn test_render_loop_stops_when_the_event_channel_closes() -> Result<()> {
    // -- Setup & Fixtures: the receiver is already dropped.
    let bus = Bus::default();
    let bus_for_loop = bus.clone();
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let cancel_controller = CancelController::default();
    let (event_tx, event_rx) = mpsc::channel::<OrchestratorEvent>(8);
    drop(event_rx);
    let (terminal_tx, terminal_rx) = mpsc::channel::<TerminalInput>(8);
    let (_branch_tx, branch_rx) = mpsc::channel::<()>(8);
    let mut live: Option<&mut ratatui::Terminal<ratatui::backend::TestBackend>> = None;

    let loop_handle = tokio::spawn(async move {
        run_render_loop(
            &mut coordinator,
            &cancel_controller,
            event_tx,
            terminal_rx,
            &mut live,
            crate::interactive::RenderLoopChannels {
                branch_rx,
                theme_persist_tx: None,
                stream: bus_for_loop.stream(),
                narration: bus_for_loop.narration(),
                control: bus_for_loop.control(),
                lifecycle: bus_for_loop.lifecycle(),
                facts: bus_for_loop.facts(),
            },
        )
        .await;
    });

    // -- Exec: one submit whose delivery cannot succeed.
    for key in [
        TerminalKey::Char('h'),
        TerminalKey::Char('i'),
        TerminalKey::Enter,
    ] {
        terminal_tx
            .send(stamped(TerminalEvent::Key(key)))
            .await
            .map_err(|_| "should send the submit keys")?;
    }

    // -- Check: the loop returns instead of waiting on a dead consumer.
    tokio::time::timeout(Duration::from_secs(1), loop_handle)
        .await
        .map_err(|_| "a closed event channel must end the loop")?
        .map_err(|err| format!("the loop task must not panic: {err}"))?;
    Ok(())
}

/// WHEN quit cannot be delivered because the channel is full, THE loop SHALL
/// keep running while it retries, and SHALL stop once the give-up deadline
/// passes.
#[tokio::test(start_paused = true)]
async fn test_render_loop_retries_a_blocked_quit_until_the_deadline() -> Result<()> {
    // -- Setup & Fixtures: one slot, already taken, and the quit already asked.
    let bus = Bus::default();
    let bus_for_loop = bus.clone();
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.state.quit_requested = true;
    coordinator.quit_requested = true;
    let cancel_controller = CancelController::default();
    let (event_tx, mut event_rx) = mpsc::channel::<OrchestratorEvent>(1);
    let (terminal_tx, terminal_rx) = mpsc::channel::<TerminalInput>(8);
    let (_branch_tx, branch_rx) = mpsc::channel::<()>(8);
    let mut live: Option<&mut ratatui::Terminal<ratatui::backend::TestBackend>> = None;

    event_tx
        .send(OrchestratorEvent::CancelRequested)
        .await
        .map_err(|_| "should fill the event channel")?;

    let loop_handle = tokio::spawn(async move {
        run_render_loop(
            &mut coordinator,
            &cancel_controller,
            event_tx,
            terminal_rx,
            &mut live,
            crate::interactive::RenderLoopChannels {
                branch_rx,
                theme_persist_tx: None,
                stream: bus_for_loop.stream(),
                narration: bus_for_loop.narration(),
                control: bus_for_loop.control(),
                lifecycle: bus_for_loop.lifecycle(),
                facts: bus_for_loop.facts(),
            },
        )
        .await;
    });
    tokio::task::yield_now().await;

    // -- Exec: one tick retries the quit and arms the deadline.
    tokio::time::advance(RENDER_TICK).await;
    tokio::task::yield_now().await;

    // -- Check: the loop is still alive while the retry runs.
    assert!(
        !loop_handle.is_finished(),
        "a full channel must keep the loop running while it retries"
    );

    // -- Exec: advance past the give-up deadline.
    tokio::time::advance(Duration::from_secs(3)).await;
    tokio::task::yield_now().await;

    // -- Check: the loop gave up and exited.
    tokio::time::timeout(Duration::from_secs(1), loop_handle)
        .await
        .map_err(|_| "the give-up deadline must end the loop")?
        .map_err(|err| format!("the loop task must not panic: {err}"))?;
    let _ = event_rx.try_recv();
    drop(terminal_tx);
    Ok(())
}
