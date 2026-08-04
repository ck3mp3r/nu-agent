use std::sync::{Arc, Mutex};

use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use crate::tools::authz::{PermissionsConfig, PermissionsOverlay, SessionGrantCache};

/// Tracks effective permissions state, the base config (without persona overlays),
/// and the CLI overlay so that agent persona switches preserve the correct precedence:
///
///   base → persona overlay → CLI overlay → effective
pub struct PermissionState {
    /// Raw plugin config without any persona overlays.
    base: PermissionsConfig,
    /// Current effective permissions (base + persona overlay + CLI overlay).
    permissions: PermissionsConfig,
    /// CLI `--permissions` overlay, re-applied after every persona switch so it
    /// always has highest precedence.
    cli_overlay: Option<PermissionsOverlay>,
    session_grants: Arc<Mutex<SessionGrantCache>>,
    startup_summary: String,
    startup_emitted: bool,
}

impl PermissionState {
    pub fn new(
        base: PermissionsConfig,
        permissions: PermissionsConfig,
        cli_overlay: Option<PermissionsOverlay>,
        session_grants: SessionGrantCache,
        startup_summary: String,
    ) -> Self {
        Self {
            base,
            permissions,
            cli_overlay,
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

    pub fn set_permissions(&mut self, permissions: PermissionsConfig, startup_summary: String) {
        self.permissions = permissions;
        self.startup_summary = startup_summary;
        // Set startup_emitted = false so the next execute_turn naturally emits
        // the new summary via emit_startup_summary_once.
        self.startup_emitted = false;
    }

    /// Apply a persona overlay during agent switch, preserving the correct
    /// precedence chain: base → persona overlay → CLI overlay → effective.
    ///
    /// Unlike `set_permissions`, this builds from `self.base` and re-applies
    /// `self.cli_overlay` so that the CLI `--permissions` flag always wins.
    pub fn with_agent_overlay(&mut self, overlay: &PermissionsOverlay) {
        let mut config = self.base.clone();
        config = config.with_overlay(overlay);
        if let Some(ref cli) = self.cli_overlay {
            config = config.with_overlay(cli);
        }
        let summary = config.summary();
        self.permissions = config;
        self.startup_summary = format!(
            "permissions policy (switch): overlay_active=true global={} tool_rules={}",
            summary.global.as_str(),
            summary.tool_rule_count,
        );
        // Reset so the next execute_turn emits the summary immediately.
        self.startup_emitted = false;
    }

    /// Clear all session grants. Used when switching agents so that
    /// "Allow always" grants from the previous agent do not persist.
    pub fn clear_session_grants(&self) {
        self.session_grants
            .lock()
            .expect("session_grants lock")
            .clear();
    }

    /// Clear session grants for tools belonging to a specific MCP server.
    /// Used when disabling an MCP server so that its tool grants are revoked.
    pub fn clear_session_grants_for_server(&self, server_name: &str) {
        self.session_grants
            .lock()
            .expect("session_grants lock")
            .clear_for_server(server_name);
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

#[cfg(test)]
#[path = "permission_test.rs"]
mod permission_test;
