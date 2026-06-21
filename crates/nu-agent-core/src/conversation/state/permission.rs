use std::sync::{Arc, Mutex};

use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use crate::tools::authz::{PermissionsConfig, SessionGrantCache};

pub struct PermissionState {
    permissions: PermissionsConfig,
    session_grants: Arc<Mutex<SessionGrantCache>>,
    startup_summary: String,
    startup_emitted: bool,
}

impl PermissionState {
    pub fn new(
        permissions: PermissionsConfig,
        session_grants: SessionGrantCache,
        startup_summary: String,
    ) -> Self {
        Self {
            permissions,
            session_grants: Arc::new(Mutex::new(session_grants)),
            startup_summary,
            startup_emitted: false,
        }
    }

    pub fn permissions(&self) -> &PermissionsConfig {
        &self.permissions
    }

    /// Returns a clone of the `Arc` — both caller and `PermissionState` share
    /// the same underlying `SessionGrantCache`. Writes made through any clone
    /// (e.g., `insert_allow_always`) are immediately visible everywhere.
    pub fn session_grants_arc(&self) -> Arc<Mutex<SessionGrantCache>> {
        Arc::clone(&self.session_grants)
    }

    pub fn emit_startup_summary_once<U: ProgressUi>(&mut self, ui: &mut U) {
        if !self.startup_emitted {
            ui.emit(&UiEvent::Warning {
                message: self.startup_summary.clone(),
            });
            self.startup_emitted = true;
        }
    }
}
