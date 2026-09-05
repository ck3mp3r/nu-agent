//! Turn domain: turn completion — finalize the cycle and close the transcript
//! block.

use nu_agent_core::bus::TurnEvent;

use super::transcript_store::TranscriptStore;
use super::{AppState, StatusState};

/// Turn-domain decisions extracted from the former `reduce_ui_event_impl`
/// `Completed` arm and `finalize` helper. The prompt-cycle completion itself
/// (prompt queue + phase) stays with `AppState::finalize_cycle`, which the
/// dispatcher invokes before routing the event here.
#[derive(Debug, Clone, Default)]
pub struct TurnState;

impl TurnState {
    /// Reduce a turn lifecycle event. Returns whether the TUI changed.
    pub fn reduce_turn_event(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        input_locked: &mut bool,
        event: TurnEvent,
    ) -> bool {
        match event {
            // Turn start and A2A task completion are not rendered in the TUI.
            TurnEvent::Started { .. } | TurnEvent::TaskCompleted { .. } => false,
            TurnEvent::Completed { .. } => self.finalize(store, status, input_locked),
        }
    }

    fn finalize(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        input_locked: &mut bool,
    ) -> bool {
        store.push_spacer();
        *input_locked = false;
        status.message.status_line.clear();
        // Reset streaming state when the LLM response is complete
        store.assistant_stream_start = None;
        true
    }
}

/// Single dispatch seam for the turn domain. A completed turn finalizes the
/// prompt cycle (prompt queue + phase) first, then closes the transcript
/// block — the exact sequence of the former `finalize` helper.
pub(crate) fn dispatch_turn_event(state: &mut AppState, event: TurnEvent) -> bool {
    match event {
        TurnEvent::Completed { .. } => {
            state.finalize_cycle();
            state.turn.reduce_turn_event(
                &mut state.transcript,
                &mut state.status,
                &mut state.input_locked,
                event,
            )
        }
        _ => false,
    }
}
