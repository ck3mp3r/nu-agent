use super::channel::{
    CancelTx, CompactionTx, ExternalTx, LlmTx, PermissionTx, SessionTx, ToolTx, TurnTx, UiStateTx,
    WarningTx,
};

/// Typed broadcast channels, one per event category.
///
/// Each channel carries its own event type, so the compiler enforces that,
/// for example, a `ToolEvent` can never be sent on the cancel channel.
#[derive(Clone)]
pub struct Bus {
    cancel: CancelTx,
    tool: ToolTx,
    llm: LlmTx,
    turn: TurnTx,
    session: SessionTx,
    external: ExternalTx,
    compaction: CompactionTx,
    warning: WarningTx,
    permission: PermissionTx,
    ui_state: UiStateTx,
}

impl Bus {
    pub fn cancel(&self) -> &CancelTx {
        &self.cancel
    }
    pub fn tool(&self) -> &ToolTx {
        &self.tool
    }
    pub fn llm(&self) -> &LlmTx {
        &self.llm
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
    pub fn warning(&self) -> &WarningTx {
        &self.warning
    }
    pub fn permission(&self) -> &PermissionTx {
        &self.permission
    }
    pub fn ui_state(&self) -> &UiStateTx {
        &self.ui_state
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            cancel: CancelTx::new("cancel", 64),
            tool: ToolTx::new("tool", 256),
            llm: LlmTx::new("llm", 64),
            turn: TurnTx::new("turn", 64),
            session: SessionTx::new("session", 16),
            external: ExternalTx::new("external", 64),
            compaction: CompactionTx::new("compaction", 16),
            warning: WarningTx::new("warning", 64),
            permission: PermissionTx::new("permission", 64),
            ui_state: UiStateTx::new("ui_state", 64),
        }
    }
}

pub fn create_bus() -> Bus {
    Bus::default()
}
