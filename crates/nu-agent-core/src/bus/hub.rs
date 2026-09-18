use super::channel::{CancelTx, CompactionTx, ExternalTx, SessionTx, TurnTx, UiEventTx, UiStateTx};

/// Typed broadcast channels, one per event category.
///
/// Each channel carries its own event type, so the compiler enforces that,
/// for example, a `TurnEvent` can never be sent on the cancel channel.
///
/// `ui_event` is the single channel for UI-facing events. The typed event
/// enums (`ToolEvent`, `LlmEvent`, `WarningEvent`, `PermissionEvent`) are
/// internal dispatch types in the TUI state layer, not bus channels.
#[derive(Clone)]
pub struct Bus {
    cancel: CancelTx,
    turn: TurnTx,
    session: SessionTx,
    external: ExternalTx,
    compaction: CompactionTx,
    ui_state: UiStateTx,
    ui_event: UiEventTx,
}

impl Bus {
    pub fn cancel(&self) -> &CancelTx {
        &self.cancel
    }
    pub fn turn(&self) -> &TurnTx {
        &self.turn
    }
    pub fn session(&self) -> &SessionTx {
        &self.session
    }
    pub fn external(&self) -> &ExternalTx {
        &self.external
    }
    pub fn compaction(&self) -> &CompactionTx {
        &self.compaction
    }
    pub fn ui_state(&self) -> &UiStateTx {
        &self.ui_state
    }
    pub fn ui_event(&self) -> &UiEventTx {
        &self.ui_event
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            cancel: CancelTx::new("cancel", 64),
            turn: TurnTx::new("turn", 64),
            session: SessionTx::new("session", 16),
            external: ExternalTx::new("external", 64),
            compaction: CompactionTx::new("compaction", 16),
            ui_state: UiStateTx::new("ui_state", 64),
            // Merged from four channels: tool 256 + llm 64 + warning 64 +
            // permission 64 = 448 slots. Raised to 1024 so burst traffic
            // cannot drop a `UiEvent::PermissionRequested` — a dropped
            // permission event means the prompt never opens while
            // `resolve()` blocks on its oneshot.
            ui_event: UiEventTx::new("ui_event", 1024),
        }
    }
}

pub fn create_bus() -> Bus {
    Bus::default()
}
