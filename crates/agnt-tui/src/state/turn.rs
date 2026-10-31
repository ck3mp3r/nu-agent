//! Turn domain: turn completion — finalize the cycle and close the transcript
//! block.

use super::transcript_store::TranscriptStore;
use super::{StatusMessageKind, StatusState};

/// Turn-domain state. The prompt-cycle completion itself (prompt queue +
/// phase) stays with `AppState::finalize_cycle`, which the reducer calls
/// before folding a completed turn here.
#[derive(Debug, Clone, Default)]
pub struct TurnState;

impl TurnState {
    /// Fold a `TurnCompleted` event: unlock the input and close the transcript
    /// block of the finished turn.
    pub(crate) fn finalize(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        input_locked: &mut bool,
    ) -> bool {
        // No explicit spacer: push_entry's unified rule separates the next
        // turn's entries from this one.
        *input_locked = false;
        // Clear only Neutral status messages. Warning-kind messages survive
        // turn finalize and expire via their own 15 s TTL — a repetition-stop
        // warning arrives just before `TurnEvent::Completed` and must not be
        // wiped by it.
        if status.message.kind() == StatusMessageKind::Neutral {
            status.message.clear();
        }
        // Reset streaming state when the LLM response is complete
        store.assistant_stream_start = None;
        true
    }
}
