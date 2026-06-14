use crate::agent::protocol::contracts::ProgressUi;
use crate::agent::protocol::event::UiEvent;
use crate::agent::tools::authz::{AsyncAskHook, PermissionsConfig, SessionGrantCache};

pub(crate) struct PermissionState {
    permissions: PermissionsConfig,
    session_grants: SessionGrantCache,
    ask_hook: AsyncAskHook,
    startup_summary: String,
    startup_emitted: bool,
}

impl PermissionState {
    pub(crate) fn new(
        permissions: PermissionsConfig,
        session_grants: SessionGrantCache,
        ask_hook: AsyncAskHook,
        startup_summary: String,
    ) -> Self {
        Self {
            permissions,
            session_grants,
            ask_hook,
            startup_summary,
            startup_emitted: false,
        }
    }

    pub(crate) fn permissions(&self) -> &PermissionsConfig {
        &self.permissions
    }

    /// Borrow all three permission components simultaneously, avoiding the
    /// overlapping-borrow problem that arises when calling individual accessors
    /// within the same struct literal.
    pub(crate) fn borrow_all_mut(
        &mut self,
    ) -> (
        &PermissionsConfig,
        &mut SessionGrantCache,
        &mut AsyncAskHook,
    ) {
        (
            &self.permissions,
            &mut self.session_grants,
            &mut self.ask_hook,
        )
    }

    pub(crate) fn emit_startup_summary_once<U: ProgressUi>(&mut self, ui: &mut U) {
        if !self.startup_emitted {
            ui.emit(&UiEvent::Warning {
                message: self.startup_summary.clone(),
            });
            self.startup_emitted = true;
        }
    }
}
