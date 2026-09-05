//! Test driver for the REAL production render loop.
//!
//! [`RenderLoopDriver`] spawns `run_render_loop` — the same `tokio::select!`
//! loop production runs — against a `TestBackend` terminal and a real [`Bus`],
//! so tests assert coordinator state and rendered frames through the production
//! event routing instead of a synthetic poll-and-drain harness.
//!
//! Determinism contract: the driver yields between scripted sends, so on the
//! current-thread `#[tokio::test]` runtime each event is fully processed by the
//! loop before the next is delivered. `advance_with_frame` additionally settles
//! past the render throttle so a frame lands in the buffer before the loop
//! exits.

use std::time::Duration;

use nu_agent_core::bus::{Bus, ToolEvent};
use nu_agent_core::orchestrator::OrchestratorEvent;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tokio::sync::mpsc;

use crate::interaction::cancel::CancelController;
use crate::interaction::input::TerminalEvent;
use crate::interactive::run_render_loop;
use crate::runtime::RuntimeCoordinator;
use crate::state::AppState;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Types

/// One scripted input to the render loop: a terminal event or a bus event.
pub(crate) enum DriveEvent {
    /// Routed through the loop's terminal arm, like a real keypress.
    Key(TerminalEvent),
    /// Published on the bus tool channel like a production stage does.
    Tool(ToolEvent),
}

/// Drives the real production render loop with a `TestBackend` terminal.
pub(crate) struct RenderLoopDriver {
    bus: Bus,
    cancel_controller: CancelController,
    coordinator: RuntimeCoordinator,
    terminal: Terminal<TestBackend>,
    orchestrator_events: Vec<OrchestratorEvent>,
}

// endregion: --- Types

impl RenderLoopDriver {
    /// How long `advance_with_frame` waits before closing the terminal
    /// channel, so at least one 80 ms render-timer tick fires past the 16 ms
    /// throttle window and a frame lands in the buffer before the loop exits.
    const FRAME_SETTLE: Duration = Duration::from_millis(150);

    // region: Constructors

    pub(crate) fn new(columns: u16, rows: u16) -> Self {
        Self {
            bus: Bus::default(),
            cancel_controller: CancelController::default(),
            coordinator: RuntimeCoordinator::new(columns, rows, Some(true)),
            terminal: test_terminal(columns, rows),
            orchestrator_events: Vec::new(),
        }
    }

    // endregion: --- Constructors

    // region: Accessors

    pub(crate) fn coordinator(&self) -> &RuntimeCoordinator {
        &self.coordinator
    }

    pub(crate) fn coordinator_mut(&mut self) -> &mut RuntimeCoordinator {
        &mut self.coordinator
    }

    pub(crate) fn state(&self) -> &AppState {
        &self.coordinator.state
    }

    pub(crate) fn orchestrator_events(&self) -> &[OrchestratorEvent] {
        &self.orchestrator_events
    }

    /// Drains the orchestrator events the loop emitted so far.
    pub(crate) fn take_orchestrator_events(&mut self) -> Vec<OrchestratorEvent> {
        std::mem::take(&mut self.orchestrator_events)
    }

    /// Rendered buffer content as plain text, one screen row per line.
    pub(crate) fn buffer_text(&self) -> String {
        let buffer = self.terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    // endregion: --- Accessors

    // region: Commands

    /// Sends the scripted events, lets the real loop process them, and stops
    /// the loop by closing the terminal channel. Coordinator state and the
    /// `TestBackend` buffer are observable afterwards.
    pub(crate) async fn advance(&mut self, events: &[DriveEvent]) -> Result<()> {
        self.run_loop_once(events, None).await
    }

    /// Like [`advance`](Self::advance), but settles long enough for the loop's
    /// render timer to flush a frame into the `TestBackend` buffer before the
    /// loop exits.
    pub(crate) async fn advance_with_frame(&mut self, events: &[DriveEvent]) -> Result<()> {
        self.run_loop_once(events, Some(Self::FRAME_SETTLE)).await
    }

    async fn run_loop_once(
        &mut self,
        events: &[DriveEvent],
        settle: Option<Duration>,
    ) -> Result<()> {
        let (event_tx, mut event_rx) = mpsc::channel::<OrchestratorEvent>(256);
        let (terminal_tx, terminal_rx) = mpsc::channel::<TerminalEvent>(events.len().max(1));
        let (_branch_tx, branch_rx) = mpsc::channel::<()>(8);

        let mut coordinator = std::mem::replace(
            &mut self.coordinator,
            RuntimeCoordinator::new(1, 1, Some(true)),
        );
        let mut terminal = std::mem::replace(&mut self.terminal, test_terminal(1, 1));

        let bus_for_loop = self.bus.clone();
        let cancel_controller = self.cancel_controller.clone();
        let handle = tokio::spawn(async move {
            let mut live = Some(&mut terminal);
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
            (coordinator, terminal)
        });

        // Yield once so the spawned loop subscribes to every bus channel
        // before the first scripted send.
        tokio::task::yield_now().await;

        for event in events {
            let delivered = match event {
                DriveEvent::Key(key) => terminal_tx.send(key.clone()).await.is_ok(),
                DriveEvent::Tool(event) => self.bus.tool().send(event.clone()).await.is_ok(),
            };
            if !delivered {
                // The loop has exited (for example on quit); later scripted
                // events cannot reach it.
                break;
            }
            // Yield so the loop processes this event before the next scripted
            // send (deterministic on the current-thread test runtime).
            tokio::task::yield_now().await;
        }

        if let Some(settle) = settle {
            tokio::time::sleep(settle).await;
        }
        drop(terminal_tx);

        let (coordinator, terminal) = handle
            .await
            .map_err(|error| format!("render loop task failed: {error}"))?;
        self.coordinator = coordinator;
        self.terminal = terminal;
        while let Some(event) = event_rx.recv().await {
            self.orchestrator_events.push(event);
        }
        Ok(())
    }

    // endregion: --- Commands
}

// region:    --- Support

fn test_terminal(columns: u16, rows: u16) -> Terminal<TestBackend> {
    match Terminal::new(TestBackend::new(columns, rows)) {
        Ok(terminal) => terminal,
        // TestBackend terminal construction is infallible: the error type is
        // uninhabited.
        Err(never) => match never {},
    }
}

// endregion: --- Support

// region:    --- Tests

#[tokio::test]
async fn render_loop_driver_test_backend_buffer_observes_sentinel_after_event() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec & Check: before any scripted event the buffer must not contain
    // the sentinel.
    driver.advance(&[]).await?;
    let before = driver.buffer_text();
    assert!(
        !before.contains("SENTINEL"),
        "sentinel must be absent before the event, got:\n{before}"
    );

    // -- Exec: type + submit the sentinel through the real terminal arm.
    let mut script: Vec<DriveEvent> = "SENTINEL"
        .chars()
        .map(|c| {
            DriveEvent::Key(TerminalEvent::Key(
                crate::interaction::input::TerminalKey::Char(c),
            ))
        })
        .collect();
    script.push(DriveEvent::Key(TerminalEvent::Key(
        crate::interaction::input::TerminalKey::Enter,
    )));
    driver.advance_with_frame(&script).await?;

    // -- Check: the rendered frame produced by the real loop must contain the
    // sentinel user prompt.
    let after = driver.buffer_text();
    assert!(
        after.contains("SENTINEL"),
        "TestBackend buffer must contain the sentinel after the loop renders it, got:\n{after}"
    );
    Ok(())
}

#[tokio::test]
async fn render_loop_driver_routes_tool_event_through_bus_tool_arm() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: publish a ToolEvent on the bus like production stages do.
    driver
        .advance(&[DriveEvent::Tool(ToolEvent::Started {
            name: "sentinel_tool".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
        })])
        .await?;

    // -- Check: the loop's tool arm reduced the event (status line set by
    // dispatch_tool_event).
    assert_eq!(
        driver.state().status.message.status_line,
        "Tool: sentinel_tool",
        "tool event must reach the coordinator through the real bus arm"
    );
    Ok(())
}

// endregion: --- Tests
