//! The surface's queues next to the picker: the info panel, the agent cycle,
//! the switch and theme requests, and the launch-request drain.

use crate::state::{AppState, InfoPanel};
use agnt_runtime::protocol::contracts::SharedUiAction;

use super::types::SwitchRequest;

impl AppState {
    pub fn open_info_panel(&mut self, panel: InfoPanel) {
        self.picker.close();
        self.info_panel = Some(panel);
        self.info_panel_scroll = 0;
    }

    pub fn close_info_panel(&mut self) {
        self.info_panel = None;
        self.info_panel_scroll = 0;
    }

    pub fn set_active_agent_identity(&mut self, name: &str) {
        self.status.identity.active_agent_identity = Some(name.to_string());
    }

    pub fn has_agents_to_cycle(&self) -> bool {
        self.status.identity.agent_cycle_names.len() >= 2
    }

    pub fn next_agent_cycle_name(&self) -> Option<String> {
        if !self.has_agents_to_cycle() {
            return None;
        }
        let current = self
            .status
            .identity
            .active_agent_identity
            .as_deref()
            .unwrap_or("");
        let current_idx = self
            .status
            .identity
            .agent_cycle_names
            .iter()
            .position(|n| n == current)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.status.identity.agent_cycle_names.len();
        Some(self.status.identity.agent_cycle_names[next_idx].clone())
    }

    pub fn queue_cycle_agent_request(&mut self) {
        if let Some(next_name) = self.next_agent_cycle_name() {
            self.pending_switch_requests
                .push_back(SwitchRequest::Agent(next_name));
        }
    }

    pub fn queue_switch_request(&mut self, req: SwitchRequest) {
        self.pending_switch_requests.push_back(req);
    }

    pub fn take_next_switch_request(&mut self) -> Option<SwitchRequest> {
        self.pending_switch_requests.pop_front()
    }

    /// Pop the next theme name the picker selected, for the command layer to
    /// persist. The TUI never writes the preference file itself.
    pub fn take_next_theme_persist_request(&mut self) -> Option<String> {
        self.pending_theme_persist.pop_front()
    }

    pub fn queue_launch_request(&mut self, action: SharedUiAction) {
        self.pending_launch.push_back(action);
        self.disarm_abort();
        self.ensure_invariants();
    }

    pub fn take_next_launch_request(&mut self) -> Option<SharedUiAction> {
        self.pending_launch.pop_front()
    }

    /// Apply the shared UI actions a fold queued.
    ///
    /// A key lens cannot re-enter the state it is dispatching from, so the
    /// action is queued and applied here, once the fold returns. The control
    /// channel's `SharedUiAction` and the local keymap shortcuts land in this
    /// one queue.
    pub fn drain_launch_requests(&mut self) -> bool {
        let mut changed = false;
        while let Some(action) = self.take_next_launch_request() {
            changed |= self.execute_shared_ui_action(action);
        }
        changed
    }
}
