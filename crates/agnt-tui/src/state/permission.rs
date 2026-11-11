use std::collections::VecDeque;

use agnt_runtime::protocol::event::{PermissionDecision, PermissionDecisionSubmission};

/// The permission surface's state: the request the open card answers, plus the
/// decisions waiting to reach the gate.
///
/// The card itself is the drawer's, derived from the `PermissionRecord` the
/// stream carried. This state holds no display data — only the identity a
/// decision or a timeout must match.
#[derive(Debug, Clone, Default)]
pub struct PermissionState {
    open_request_id: Option<String>,
    pending_decisions: VecDeque<PermissionDecisionSubmission>,
}

impl PermissionState {
    /// Open the card for `request_id`.
    pub fn open_prompt(&mut self, request_id: impl Into<String>) {
        self.open_request_id = Some(request_id.into());
    }

    pub fn has_prompt(&self) -> bool {
        self.open_request_id.is_some()
    }

    /// Close the open prompt without submitting a decision.
    ///
    /// Used when the request can no longer be answered: the turn was cancelled
    /// or has ended, so the resolver already dropped the request. Queued
    /// submissions are left untouched.
    pub fn close_prompt(&mut self) {
        self.open_request_id = None;
    }

    /// Close the open prompt only when it belongs to `request_id`.
    ///
    /// Returns whether the prompt was closed. A decision timeout for another
    /// request must leave the open card alone.
    pub fn close_prompt_if(&mut self, request_id: &str) -> bool {
        let matches_open = self.open_request_id.as_deref() == Some(request_id);
        if matches_open {
            self.open_request_id = None;
        }
        matches_open
    }

    pub fn submit_decision(&mut self, decision: PermissionDecision) -> bool {
        let Some(request_id) = self.open_request_id.take() else {
            return false;
        };
        self.pending_decisions
            .push_back(PermissionDecisionSubmission {
                request_id,
                decision,
            });
        true
    }

    pub fn take_next_submission(&mut self) -> Option<PermissionDecisionSubmission> {
        self.pending_decisions.pop_front()
    }
}
