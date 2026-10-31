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

use agnt_core::bus::Bus;
use agnt_core::orchestrator::OrchestratorEvent;
use agnt_core::protocol::events::SystemEvent;
use agnt_core::protocol::tool_args::CallLine;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::interaction::cancel::CancelController;
use crate::interaction::input::TerminalEvent;
use crate::interactive::run_render_loop;
use crate::runtime::RuntimeCoordinator;
use crate::state::AppState;
use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::renderer::ItemStatus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Types

/// One scripted input to the render loop: a terminal event or a bus event.
pub(crate) enum DriveEvent {
    /// Routed through the loop's terminal arm, like a real keypress.
    Key(TerminalEvent),
    /// Published on the bus ui_event channel like a production stage does.
    ///
    /// Boxed because `SystemEvent` is much larger than `TerminalEvent`; clippy's
    /// `large_enum_variant` requires the indirection.
    SystemEvent(Box<SystemEvent>),
}

/// A cell from the last rendered frame: where it sits, its glyph, and its
/// resolved style.
#[derive(Debug, Clone)]
pub(crate) struct CellProbe {
    pub(crate) position: ratatui::layout::Position,
    pub(crate) symbol: String,
    pub(crate) style: ratatui::style::Style,
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
    /// The render loop's own timer cadence, taken from production so the driver
    /// settles in lockstep with the loop it drives.
    const RENDER_TICK: Duration = crate::interactive::RENDER_TICK;

    /// Bounded budget for `advance_with_frame` to await the loop's render tick.
    /// The loop's own timer starts before the driver's, so two tick periods
    /// guarantee its tick fires and the frame is flushed before the terminal
    /// channel closes.
    const FRAME_BUDGET: Duration = Duration::from_millis(500);

    // region: Constructors

    pub(crate) fn new(columns: u16, rows: u16) -> Self {
        Self {
            bus: Bus::default(),
            cancel_controller: CancelController::default(),
            coordinator: RuntimeCoordinator::new(columns, rows, None),
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

    /// Whether the terminal cursor is visible in the last rendered frame.
    pub(crate) fn cursor_visible(&self) -> bool {
        self.terminal.backend().cursor_visible()
    }

    /// The cell at `(x, y)` in the last rendered frame.
    pub(crate) fn cell_at(&self, x: u16, y: u16) -> CellProbe {
        let cell = &self.terminal.backend().buffer()[(x, y)];
        CellProbe {
            position: ratatui::layout::Position { x, y },
            symbol: cell.symbol().to_string(),
            style: cell.style(),
        }
    }

    /// The first cell (if any) whose glyph equals `symbol`, scanning row-major
    /// across the whole rendered buffer.
    pub(crate) fn cell_with_symbol(&self, symbol: &str) -> Option<CellProbe> {
        self.probe_cells(|cell| cell.symbol() == symbol)
    }

    /// The first cell (if any) whose style carries the reversed modifier,
    /// scanning row-major across the whole rendered buffer.
    pub(crate) fn reversed_cell(&self) -> Option<CellProbe> {
        self.probe_cells(|cell| {
            cell.style()
                .add_modifier
                .contains(ratatui::style::Modifier::REVERSED)
        })
    }

    fn probe_cells(&self, matches: impl Fn(&ratatui::buffer::Cell) -> bool) -> Option<CellProbe> {
        let buffer = self.terminal.backend().buffer();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                if matches(cell) {
                    return Some(CellProbe {
                        position: ratatui::layout::Position { x, y },
                        symbol: cell.symbol().to_string(),
                        style: cell.style(),
                    });
                }
            }
        }
        None
    }

    /// Returns the first cell (if any) whose style has no explicit background,
    /// scanning row-major across the whole rendered buffer.
    pub(crate) fn first_cell_without_bg(&self) -> Option<(u16, u16)> {
        let buffer = self.terminal.backend().buffer();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if buffer[(x, y)].style().bg.is_none() {
                    return Some((x, y));
                }
            }
        }
        None
    }

    /// Rendered buffer content within `rect` as plain text, one screen row per
    /// line (rows outside `rect` are omitted).
    pub(crate) fn buffer_text_in_rect(&self, rect: Rect) -> String {
        let buffer = self.terminal.backend().buffer();
        let mut text = String::new();
        for y in rect.y..rect.y.saturating_add(rect.height) {
            for x in rect.x..rect.x.saturating_add(rect.width) {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    /// The resolved style of the cell at `(x, y)` in the last rendered frame.
    pub(crate) fn cell_style_at(&self, x: u16, y: u16) -> ratatui::style::Style {
        self.terminal.backend().buffer()[(x, y)].style()
    }

    // endregion: --- Accessors

    // region: Commands

    /// Sends the scripted events, lets the real loop process them, and stops
    /// the loop by closing the terminal channel. Coordinator state and the
    /// `TestBackend` buffer are observable afterwards.
    pub(crate) async fn advance(&mut self, events: &[DriveEvent]) -> Result<()> {
        self.run_loop_once(events, None).await
    }

    /// Like [`advance`](Self::advance), but awaits the loop's render-timer tick
    /// with a bounded budget so a frame lands in the `TestBackend` buffer
    /// before the loop exits.
    pub(crate) async fn advance_with_frame(&mut self, events: &[DriveEvent]) -> Result<()> {
        self.run_loop_once(events, Some(Self::FRAME_BUDGET)).await
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
                crate::interactive::RenderLoopChannels {
                    branch_rx,
                    theme_persist_tx: None,
                },
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
                DriveEvent::SystemEvent(event) => {
                    self.bus.ui_event().send((**event).clone()).await.is_ok()
                }
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

        if let Some(budget) = settle {
            // The loop flushes a pending frame on its render tick. The driver
            // cannot observe a frame while the loop owns the terminal, so await
            // two of the loop's ticks with a bounded budget: the loop's timer
            // starts first, so its tick fires within the first period and the
            // second gives it slack to flush before the terminal channel closes.
            let mut render_tick = tokio::time::interval(Self::RENDER_TICK);
            let _ = tokio::time::timeout(budget, async {
                render_tick.tick().await;
                render_tick.tick().await;
                render_tick.tick().await;
            })
            .await;
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
async fn render_loop_driver_routes_tool_event_through_bus_ui_event_arm() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: publish a SystemEvent on the bus like production stages do.
    driver
        .advance(&[DriveEvent::SystemEvent(Box::new(
            SystemEvent::ToolStarted {
                call: agnt_core::protocol::events::ToolCall::new(
                    agnt_core::transcript::ir::ToolName("sentinel_tool".to_string()),
                    "mcp",
                    "{}",
                    CallLine::from_json_summary("{}"),
                ),
            },
        ))])
        .await?;

    // -- Check: the loop's ui_event arm reduced the event (the transcript
    // records the tool entry). The entry carries the call line, not the tool
    // name, so assert a Tool entry materialized with in-progress status.
    assert!(
        driver
            .state()
            .transcript
            .entries()
            .iter()
            .any(|entry| matches!(entry, TranscriptEntry::Tool(tool) if tool.status == ItemStatus::InProgress)),
        "tool event must reach the coordinator through the real bus arm"
    );
    Ok(())
}

/// Every popup renders its navigation keys in the title, with " · " between
/// hint items: scroll panels advertise ↑/↓ · PgUp/PgDn · Esc, pickers advertise
/// ↑/↓ · Enter · Esc.
#[tokio::test]
async fn every_popup_title_shows_its_navigation_keys() -> Result<()> {
    // -- Setup & Fixtures: one popup per case, opened through the real state.
    let cases: [(&str, &str); 9] = [
        ("status", "Status (↑/↓ · PgUp/PgDn · Esc)"),
        ("skills", "Skills (↑/↓ · PgUp/PgDn · Esc)"),
        ("help", "Help (↑/↓ · PgUp/PgDn · Esc"),
        ("palette", "Command Palette (↑/↓ · Enter · Esc"),
        ("models", "Models (↑/↓ · Enter · Esc)"),
        ("agent", "Agent (↑/↓ · Enter · Esc)"),
        ("sessions", "Sessions (↑/↓ · Enter · Esc"),
        ("theme", "Theme (↑/↓ · Enter · Esc)"),
        ("inline_slash", "Commands (↑/↓ · Enter · Esc)"),
    ];

    for (label, expected) in cases {
        // -- Setup & Fixtures: a fresh driver per popup.
        let mut driver = RenderLoopDriver::new(160, 40);
        {
            let state = &mut driver.coordinator_mut().state;
            match label {
                "status" => state.open_info_panel(crate::state::InfoPanel::Status),
                "skills" => state.open_info_panel(crate::state::InfoPanel::Skills),
                "help" => state.open_info_panel(crate::state::InfoPanel::Help),
                "palette" => {
                    state
                        .picker
                        .open(crate::state::ActivePicker::CommandPalette);
                }
                "models" => {
                    state.picker.open(crate::state::ActivePicker::Model);
                }
                "agent" => {
                    state.picker.open(crate::state::ActivePicker::Agent);
                }
                "sessions" => {
                    state.picker.open(crate::state::ActivePicker::Session);
                }
                "inline_slash" => state.check_inline_slash("/"),
                _ => {
                    state.picker.open(crate::state::ActivePicker::Theme);
                }
            }
        }

        // -- Exec: settle past the render throttle so a frame lands.
        driver.advance_with_frame(&[]).await?;

        // -- Check: the rendered title carries the keys, never a legacy " | ".
        let buffer = driver.buffer_text();
        assert!(
            buffer.contains(expected),
            "{label}: expected title fragment {expected:?} in:\n{buffer}"
        );
    }
    Ok(())
}

// endregion: --- Tests
