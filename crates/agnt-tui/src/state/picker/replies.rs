//! The picker's reply hands: the refresh requests the orchestrator answers, the
//! drain that applies the answers, and the row install both catalogs land in.

use crate::state::AppState;
use agnt_runtime::orchestrator::events::{
    PickerReply, RefreshAgentPickerResult, RefreshSessionPickerResult,
};

use super::rows::sort_picker_options;
use super::types::{ActivePicker, PickerOption};

impl AppState {
    pub fn set_picker_options<T: Into<PickerOption>>(
        &mut self,
        kind: ActivePicker,
        options: Vec<T>,
    ) {
        let mut options: Vec<PickerOption> = options.into_iter().map(Into::into).collect();
        sort_picker_options(&mut options);
        let idx = self.picker.index_of(kind);
        self.picker.entries[idx].state.options = options;
        self.ensure_invariants();
    }

    /// Take the pending session-picker refresh request, if one is queued.
    ///
    /// Repeated opens before a drain collapse into one request. The returned
    /// hand is the one the catalog comes back on: the catalog is surface
    /// state, so the orchestrator answers the surface that asked and nothing
    /// reaches a bus channel. [`poll_picker_replies`](Self::poll_picker_replies)
    /// drains the answer.
    pub fn take_next_session_picker_refresh_request(
        &mut self,
    ) -> Option<PickerReply<RefreshSessionPickerResult>> {
        std::mem::take(&mut self.pending_session_picker_refresh)
            .then(|| self.session_picker_tx.clone())
    }

    /// Take the pending agent-picker refresh request, if one is queued.
    /// Repeated opens before a drain collapse into one request.
    pub fn take_next_agent_picker_refresh_request(
        &mut self,
    ) -> Option<PickerReply<RefreshAgentPickerResult>> {
        std::mem::take(&mut self.pending_agent_picker_refresh).then(|| self.agent_picker_tx.clone())
    }

    /// Drain every picker answer that arrived since the last poll.
    ///
    /// Both failure modes end the drain: an empty hand has nothing more to
    /// deliver, and a closed one never will.
    ///
    /// Returns whether any answer changed the rendered state.
    pub fn poll_picker_replies(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.session_picker_rx.try_recv() {
            self.apply_session_picker_result(result);
            changed = true;
        }
        while let Ok(result) = self.agent_picker_rx.try_recv() {
            self.apply_agent_picker_result(result);
            changed = true;
        }
        changed
    }

    /// The session catalog this surface received, as picker rows. A failed
    /// scan reaches the user as the warning the picker would have shown.
    fn apply_session_picker_result(&mut self, result: RefreshSessionPickerResult) {
        match result {
            Ok(sessions) => {
                let options: Vec<PickerOption> =
                    sessions.into_iter().map(PickerOption::from).collect();
                self.set_picker_options(ActivePicker::Session, options);
            }
            Err(message) => self.status.message.set_warning(message),
        }
    }

    /// The agent catalog this surface received. One scan feeds both surfaces:
    /// the picker rows and the Tab cycle.
    fn apply_agent_picker_result(&mut self, result: RefreshAgentPickerResult) {
        match result {
            Ok(summaries) => {
                self.status.identity.agent_cycle_names =
                    summaries.iter().map(|s| s.name.clone()).collect();
                let catalog =
                    agnt_runtime::protocol::picker::build_agent_picker_catalog(&summaries);
                let options: Vec<PickerOption> =
                    catalog.into_iter().map(PickerOption::from).collect();
                self.set_picker_options(ActivePicker::Agent, options);
            }
            Err(message) => self.status.message.set_warning(message),
        }
    }
}
