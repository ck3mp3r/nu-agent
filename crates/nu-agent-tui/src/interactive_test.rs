use std::time::Duration;

use nu_agent_core::bus::{Bus, TurnEvent};
use nu_agent_core::orchestrator::OrchestratorEvent;
use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::renderer::UiRenderer;
use tokio::sync::mpsc;

use crate::interaction::cancel::CancelController;
use crate::interaction::input::{TerminalEvent, TerminalKey};
use crate::runtime::{HybridTerminalEvents, RuntimeCoordinator};

use super::{TuiInteractiveUi, run_render_loop};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

struct FakeRenderer;

impl UiRenderer for FakeRenderer {
    fn emit(&mut self, _event: &UiEvent) {}
    fn flush(&mut self) {}
}

fn make_ui() -> TuiInteractiveUi<FakeRenderer> {
    let events = HybridTerminalEvents::new(Duration::from_millis(60), None);
    let renderer = crate::runtime::TuiRuntimeRenderer::new(FakeRenderer, events, 120, 30);
    TuiInteractiveUi::new(renderer)
}

#[test]
fn set_active_persona_icon_updates_state() {
    let mut ui = make_ui();

    ui.set_active_persona_icon(Some("icon".to_string()));

    assert_eq!(
        ui.renderer
            .coordinator
            .state
            .status
            .active_persona_icon
            .as_deref(),
        Some("icon")
    );
}

#[test]
fn set_active_persona_icon_clears_state_when_none() {
    let mut ui = make_ui();

    ui.set_active_persona_icon(Some("icon".to_string()));
    ui.set_active_persona_icon(None);

    assert_eq!(
        ui.renderer.coordinator.state.status.active_persona_icon,
        None
    );
}

#[test]
fn permission_event_requested_opens_permission_prompt() -> Result<()> {
    let mut ui = make_ui();
    let event = nu_agent_core::bus::PermissionEvent::Requested {
        request_id: "ask-0000000000000001".to_string(),
        context: Box::new(nu_agent_core::protocol::event::PermissionRequestContext {
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
            pre_authorize_display: None,
        }),
    };
    assert!(
        ui.renderer
            .coordinator
            .state
            .permission
            .reduce_permission_event(event)
    );
    assert!(ui.renderer.coordinator.state.permission.has_prompt());
    Ok(())
}

#[tokio::test]
async fn turn_completion_drains_stacked_prompts_without_terminal_input() -> Result<()> {
    let bus = Bus::default();
    let bus_for_loop = bus.clone();
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let cancel_controller = CancelController::default();
    let (event_tx, mut event_rx) = mpsc::channel::<OrchestratorEvent>(64);
    let (terminal_tx, terminal_rx) = mpsc::channel::<TerminalEvent>(64);
    let (_branch_tx, branch_rx) = mpsc::channel::<()>(8);
    let mut live = None;

    let loop_handle = tokio::spawn(async move {
        run_render_loop(
            &mut coordinator,
            &bus_for_loop,
            &cancel_controller,
            event_tx,
            terminal_rx,
            &mut live,
            branch_rx,
        )
        .await;
    });

    // Submit the first prompt and let the render loop drain it.
    for key in [TerminalKey::Char('h'), TerminalKey::Char('i')] {
        terminal_tx
            .send(TerminalEvent::Key(key))
            .await
            .expect("send char");
    }
    terminal_tx
        .send(TerminalEvent::Key(TerminalKey::Enter))
        .await
        .expect("submit first");
    let first = event_rx
        .recv()
        .await
        .ok_or("should receive first PromptSubmitted")?;
    assert!(
        matches!(&first, OrchestratorEvent::PromptSubmitted { text } if text == "hi"),
        "first submitted prompt must be 'hi'"
    );

    // Stack a second prompt while the turn is still busy. This is enqueued to
    // pending_prompt_ids but must NOT be drained yet (active prompt still set).
    for key in [TerminalKey::Char('x')] {
        terminal_tx
            .send(TerminalEvent::Key(key))
            .await
            .expect("send x");
    }
    terminal_tx
        .send(TerminalEvent::Key(TerminalKey::Enter))
        .await
        .expect("submit second");
    // Give the loop a moment to process the second submit without completing.
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        event_rx.try_recv().is_err(),
        "second prompt must not drain while the turn is still active"
    );

    // Complete the turn. The turn-completion branch must drain the stacked
    // prompt into a PromptSubmitted event without any further terminal input.
    bus.turn()
        .send(TurnEvent::Completed { tool_calls: 0 })
        .await
        .expect("publish TurnCompleted");
    let second = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
        .await
        .map_err(|_| "should receive second PromptSubmitted before timeout")?
        .ok_or("should receive second PromptSubmitted")?;
    assert!(
        matches!(&second, OrchestratorEvent::PromptSubmitted { text } if text == "x"),
        "stacked prompt must be drained on turn completion"
    );

    drop(terminal_tx);
    let _ = tokio::time::timeout(Duration::from_millis(500), loop_handle).await;
    Ok(())
}
