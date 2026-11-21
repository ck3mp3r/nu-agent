use std::time::{Duration, Instant};

use agnt_render::transcript::ir::ToolName;
use agnt_runtime::bus::{Bus, LifecycleEvent};
use agnt_runtime::orchestrator::events::OrchestratorEvent;
use agnt_runtime::protocol::contracts::UiMessageSnapshot;
use agnt_runtime::protocol::events::{
    PermissionRecord, RequestId, SessionRecord, ToolCall, ToolRecord, TurnRecord,
};
use tokio::sync::mpsc;

use crate::interaction::cancel::CancelController;
use crate::interaction::input::{TerminalEvent, TerminalInput, TerminalKey};
use crate::runtime::RuntimeCoordinator;
use crate::test_support::entry_text;

use super::{STREAM_DRAIN_MAX, TuiInteractiveUi, run_render_loop};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Modules

#[path = "interactive/drain_bound.rs"]
mod drain_bound;

#[path = "interactive/delivery.rs"]
mod delivery;

// endregion: --- Modules

fn make_ui() -> TuiInteractiveUi {
    TuiInteractiveUi::new(crate::runtime::TuiRuntimeRenderer::new(120, 30))
}

/// One terminal event stamped with the instant it is sent.
fn stamped(event: TerminalEvent) -> TerminalInput {
    TerminalInput {
        event,
        read_at: Instant::now(),
    }
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
            .identity
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
        ui.renderer
            .coordinator
            .state
            .status
            .identity
            .active_persona_icon,
        None
    );
}

#[test]
fn set_mcp_visible_tool_count_by_server_name_updates_state() {
    let mut ui = make_ui();

    ui.set_mcp_visible_tool_count_by_server_name("c5t", 37);

    assert_eq!(
        ui.renderer
            .coordinator
            .state
            .status
            .mcp
            .mcp_visible_tool_count_for_server_name("c5t"),
        37
    );
}

#[test]
fn set_mcp_visible_tool_names_by_server_name_sorts_and_dedups() {
    let mut ui = make_ui();

    ui.set_mcp_visible_tool_names_by_server_name(
        "c5t",
        vec![
            "task_list".to_string(),
            "task_get".to_string(),
            "task_list".to_string(),
        ],
    );

    assert_eq!(
        ui.renderer
            .coordinator
            .state
            .status
            .mcp
            .mcp_visible_tool_names_for_server_name("c5t"),
        vec!["task_get".to_string(), "task_list".to_string()]
    );
}

#[test]
fn hydrate_transcript_restores_the_persisted_messages_and_tokens_once() {
    let mut ui = make_ui();

    ui.hydrate_transcript(
        vec![
            UiMessageSnapshot::new("user", "restored question"),
            UiMessageSnapshot::new("assistant", "restored answer"),
        ],
        Some(1234),
    );

    let entries = ui.renderer.coordinator.state.transcript.entries();
    assert_eq!(
        entries.len(),
        2,
        "each restored message renders exactly once"
    );
    assert_eq!(entry_text(&entries[0]), "restored question");
    assert_eq!(entry_text(&entries[1]), "restored answer");
    assert_eq!(
        ui.renderer
            .coordinator
            .state
            .status
            .tokens
            .latest_total_tokens,
        Some(1234)
    );
}

#[test]
fn hydrate_transcript_with_no_messages_leaves_the_transcript_empty() {
    let mut ui = make_ui();

    ui.hydrate_transcript(Vec::new(), None);

    assert!(
        ui.renderer
            .coordinator
            .state
            .transcript
            .entries()
            .is_empty()
    );
    assert_eq!(
        ui.renderer
            .coordinator
            .state
            .status
            .tokens
            .latest_total_tokens,
        None
    );
}

#[test]
fn test_render_loop_log_lag_composes_lane_and_count() {
    // -- Exec
    let message = super::render_loop::log_lag("stream", 5);

    // -- Check
    assert!(
        message.contains("tui-lag: stream lane lagged: 5 record(s)"),
        "the lag line must name the lane and the drop count: {message}"
    );
}

#[test]
fn permission_event_requested_opens_permission_prompt() -> Result<()> {
    let mut ui = make_ui();
    let event = SessionRecord::Permission(PermissionRecord::Requested {
        request_id: RequestId::from_minted("ask-0000000000000001"),
        call_id: agnt_runtime::protocol::events::ToolCallId::from_key(
            "nu\n{\"command\":\"echo hi\"}",
        ),
        tool: "nu(command=echo hi)".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        rule_identity: "nested:nu.command:*".to_string(),
        scope: "nested".to_string(),
        target_field: Some("command".to_string()),
        pattern: "*".to_string(),
        summary: "→ {\"command\":\"echo hi\"}".to_string(),
        shows_preview: true,
        preview: None,
    });
    assert!(ui.renderer.coordinator.fold_record(&event));
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
    let (terminal_tx, terminal_rx) = mpsc::channel::<TerminalInput>(64);
    let (_branch_tx, branch_rx) = mpsc::channel::<()>(8);
    // The loop only needs the terminal for its backend type; no live terminal is
    // opened, so the test backend stands in for the crossterm one.
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

    // Submit the first prompt and let the render loop drain it.
    for key in [TerminalKey::Char('h'), TerminalKey::Char('i')] {
        terminal_tx
            .send(stamped(TerminalEvent::Key(key)))
            .await
            .map_err(|_| "should send char")?;
    }
    terminal_tx
        .send(stamped(TerminalEvent::Key(TerminalKey::Enter)))
        .await
        .map_err(|_| "should submit first")?;
    let first = event_rx
        .recv()
        .await
        .ok_or("should receive first PromptSubmitted")?;
    assert!(
        matches!(&first, OrchestratorEvent::PromptSubmitted { text, .. } if text == "hi"),
        "first submitted prompt must be 'hi'"
    );

    // Stack a second prompt while the turn is still busy. This is enqueued to
    // pending_prompt_ids but must NOT be drained yet (active prompt still set).
    for key in [TerminalKey::Char('x')] {
        terminal_tx
            .send(stamped(TerminalEvent::Key(key)))
            .await
            .map_err(|_| "should send x")?;
    }
    terminal_tx
        .send(stamped(TerminalEvent::Key(TerminalKey::Enter)))
        .await
        .map_err(|_| "should submit second")?;
    // The stacked prompt must not drain while the turn is active. Await the
    // event channel with a bounded budget: a drain would arrive immediately,
    // expiry proves none was sent.
    let drained = tokio::time::timeout(Duration::from_millis(50), event_rx.recv()).await;
    assert!(
        drained.is_err(),
        "second prompt must not drain while the turn is still active"
    );

    // The ring's own record is display-only: it must not drain the stack while
    // the turn it describes is still the surface's live turn.
    bus.stream()
        .publish(SessionRecord::Turn(TurnRecord::Completed {
            duration_ms: 0,
        }))
        .map_err(|_| "should publish the ring record")?;
    let after_record = tokio::time::timeout(Duration::from_millis(50), event_rx.recv()).await;
    assert!(
        after_record.is_err(),
        "the lossy ring's record must not drain the stacked prompt"
    );

    // Complete the turn on the strict spine. The spine fold must drain the
    // stacked prompt into a PromptSubmitted event without any further terminal
    // input.
    bus.lifecycle()
        .publish(LifecycleEvent::TurnCompleted {
            task_id: None,
            duration_ms: 0,
        })
        .map_err(|_| "should publish TurnCompleted")?;
    let second = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
        .await
        .map_err(|_| "should receive second PromptSubmitted before timeout")?
        .ok_or("should receive second PromptSubmitted")?;
    assert!(
        matches!(&second, OrchestratorEvent::PromptSubmitted { text, .. } if text == "x"),
        "stacked prompt must be drained on turn completion"
    );

    drop(terminal_tx);
    let _ = tokio::time::timeout(Duration::from_millis(500), loop_handle).await;
    Ok(())
}

/// WHEN a terminal event's read instant is 300 ms in the past, THE loop SHALL
/// record that wait: the fold is fast, and the wait is the starvation signal.
#[tokio::test]
async fn stale_key_stamp_records_the_input_wait() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let bus_for_loop = bus.clone();
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let cancel_controller = CancelController::default();
    let (event_tx, _event_rx) = mpsc::channel::<OrchestratorEvent>(64);
    let (terminal_tx, terminal_rx) = mpsc::channel::<TerminalInput>(64);
    let (_branch_tx, branch_rx) = mpsc::channel::<()>(8);
    let mut live: Option<&mut ratatui::Terminal<ratatui::backend::TestBackend>> = None;

    // The loop hands the coordinator back when its terminal channel closes, so
    // the test can read what the loop recorded.
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

    // -- Exec: one key whose read instant is 300 ms in the past.
    let read_at = Instant::now()
        .checked_sub(Duration::from_millis(300))
        .ok_or("should construct a past instant")?;
    terminal_tx
        .send(TerminalInput {
            event: TerminalEvent::Key(TerminalKey::Char('h')),
            read_at,
        })
        .await
        .map_err(|_| "should send the stale key")?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(terminal_tx);
    let coordinator = tokio::time::timeout(Duration::from_secs(1), loop_handle)
        .await
        .map_err(|_| "the loop must exit when the terminal channel closes")?
        .map_err(|err| format!("the loop task must not panic: {err}"))?;

    // -- Check
    let wait = coordinator.arm_timing().last_input_wait_millis();
    assert!(
        wait >= 250,
        "a key read 300 ms ago must record a wait of at least 250 ms, got: {wait}"
    );
    Ok(())
}
