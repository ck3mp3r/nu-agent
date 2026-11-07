use agnt_runtime::protocol::slash::filter_inline_slash_suggestions;

use super::*;

use std::time::Instant;

use agnt_render::transcript::entry::{MessageEntry, TranscriptEntry};
use agnt_render::transcript::ir::MessageRole;

pub(crate) const CTRL_C_QUIT_CONFIRM_STATUS: &str = "Ctrl-C again to quit";

impl AppState {
    pub fn new_with_sender(
        event_tx: tokio::sync::mpsc::Sender<agnt_runtime::orchestrator::events::OrchestratorEvent>,
    ) -> Self {
        Self {
            event_tx,
            ..Self::default()
        }
    }

    pub fn is_active_cycle(&self) -> bool {
        self.active_cycle
    }

    pub fn prompt_items(&self) -> &[QueuedPrompt] {
        &self.prompt_items
    }

    pub fn pending_prompt_count(&self) -> usize {
        self.pending_prompt_ids.len()
    }

    pub fn accept_submit(&mut self) {
        self.phase = UiPhase::Busy;
        self.active_cycle = self.active_prompt_id.is_some() || !self.pending_prompt_ids.is_empty();
        self.reset_abort();
        self.ensure_invariants();
    }

    /// The footer facts a submission made now carries: the submit instant and
    /// the identities active at this boundary.
    ///
    /// This is the TUI's one sanctioned now-write — the moment a prompt is
    /// accepted, never later, and never a re-derivation of the instant.
    pub fn submit_facts(&self) -> PromptFacts {
        PromptFacts {
            millis: Some(chrono::Utc::now().timestamp_millis()),
            model: Some(self.status.identity.active_model_identity.clone()),
            agent: self.status.identity.active_agent_identity.clone(),
        }
    }

    pub fn enqueue_prompt(&mut self, submitted_text: String) -> u64 {
        self.enqueue_prompt_with_facts(submitted_text, PromptFacts::default())
    }

    /// The submit boundary's enqueue: the carrier keeps the facts it was handed.
    pub fn enqueue_prompt_with_facts(&mut self, submitted_text: String, facts: PromptFacts) -> u64 {
        let id = prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .enqueue_prompt(submitted_text, facts);
        self.accept_submit();
        id
    }

    pub fn enqueue_immediate_submission(&mut self, submitted_text: String) {
        self.enqueue_immediate_submission_with_facts(submitted_text, PromptFacts::default())
    }

    /// The submit boundary's immediate enqueue: the carrier keeps the facts it
    /// was handed.
    pub fn enqueue_immediate_submission_with_facts(
        &mut self,
        submitted_text: String,
        facts: PromptFacts,
    ) {
        self.pending_immediate_submissions
            .push_back(ImmediateSubmission {
                text: submitted_text,
                facts,
            });
        self.reset_abort();
        self.ensure_invariants();
    }

    pub fn activate_next_prompt(&mut self) -> Option<u64> {
        let maybe_id = prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .activate_next_prompt();

        if maybe_id.is_none() {
            self.ensure_invariants();
            return None;
        }

        let active_id = maybe_id?;
        let prompt = self
            .prompt_items
            .iter()
            .find(|p| p.id == active_id)
            .cloned()
            .unwrap_or_else(|| QueuedPrompt {
                id: active_id,
                prompt_text: String::new(),
                status: PromptStatus::InProgress,
                facts: PromptFacts::default(),
            });
        // The row carries the carrier's facts verbatim: what the submit boundary
        // stamped, nothing filled in.
        let msg = MessageEntry {
            role: MessageRole::User,
            markdown: prompt.prompt_text,
            timestamp_millis: prompt.facts.millis,
            model: prompt.facts.model,
            agent: prompt.facts.agent,
        };
        let evicted = self.transcript.push_entry(TranscriptEntry::Message(msg));
        self.shift_bookkeeping_after_eviction(evicted);

        self.phase = UiPhase::Busy;
        self.active_cycle = true;
        self.reset_abort();
        self.ensure_invariants();
        Some(active_id)
    }

    pub fn take_next_prompt_for_execution(&mut self) -> Option<String> {
        if let Some(immediate) = self.pending_immediate_submissions.pop_front() {
            // The immediate lane runs commands, not prose: it never pushes a row
            // of its own (the slash-command contract pins an untouched
            // transcript). Its facts ride the carrier and stop here.
            self.ensure_invariants();
            return Some(immediate.text);
        }

        let (texts, facts) = prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .coalesce_pending_prompts();

        if texts.is_empty() {
            self.ensure_invariants();
            return None;
        }

        // One newline is a markdown soft break: the projector flushes each
        // break as a row break, so the submitted block renders one row per
        // queued message — the stack's rhythm, with no blank row between them
        // (pinned by `layout_coalesced_user_block_renders_one_row_per_message`).
        let combined = texts.join("\n");
        // The coalesced block carries the first `Some` of each fact, in queue
        // order: a merged row keeps what its first stamped prompt knew.
        let msg = MessageEntry {
            role: MessageRole::User,
            markdown: combined.clone(),
            timestamp_millis: facts.millis,
            model: facts.model,
            agent: facts.agent,
        };
        let evicted = self.transcript.push_entry(TranscriptEntry::Message(msg));
        self.shift_bookkeeping_after_eviction(evicted);

        self.phase = UiPhase::Busy;
        self.active_cycle = true;
        self.reset_abort();
        self.ensure_invariants();
        Some(combined)
    }

    pub fn complete_active_prompt(&mut self) {
        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .complete_active_prompt();

        // The settle that lands between the arming press and the confirming one:
        // the arm is dropped without a cancel, so the next idle Esc owes the
        // user the settled notice.
        if self.phase == UiPhase::AbortPending && self.abort.pending {
            self.abort.settle_notice = true;
        }
        self.phase = UiPhase::Idle;
        self.input_locked = false;
        self.active_cycle = false;
        self.disarm_abort();
        self.ensure_invariants();
    }

    pub fn request_abort_confirmation(&mut self) -> bool {
        if !(self.active_prompt_id.is_some()
            || !self.pending_prompt_ids.is_empty()
            || self.active_cycle)
        {
            self.ensure_invariants();
            return false;
        }

        self.abort.pending = true;
        self.abort.armed_at = Some(Instant::now());
        self.abort.confirmation_marker = self.abort.confirmation_marker.saturating_add(1);
        self.abort.settle_notice = false;
        self.phase = UiPhase::AbortPending;
        self.ensure_invariants();
        true
    }

    /// Drop an arm whose message lifetime is over: silently, with the turn back
    /// in its busy state when a cycle still runs, otherwise idle. The settle
    /// notice is not owed — an expired arm was never lost to a settle.
    ///
    /// Returns whether the arm was dropped.
    pub fn sweep_expired_abort(&mut self, now: Instant) -> bool {
        let Some(armed_at) = self.abort.armed_at else {
            return false;
        };
        if now.saturating_duration_since(armed_at) < self.abort.arm_window {
            return false;
        }
        self.disarm_abort();
        self.phase = if self.active_cycle {
            UiPhase::Busy
        } else {
            UiPhase::Idle
        };
        true
    }

    pub fn finalize_cycle(&mut self) {
        self.complete_active_prompt();
    }

    /// Returns the restored text from cancelled pending prompts, or None.
    /// The caller is responsible for setting the textarea content.
    /// Also stores the result in `input.restored_input_text` for the coordinator
    /// to pick up on the next pump cycle.
    pub fn cancel_and_restore_pending_to_input(&mut self) -> Option<String> {
        let texts = prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .cancel_and_drain_to_texts();

        let result = if !texts.is_empty() {
            Some(texts.join("\n\n"))
        } else {
            None
        };

        self.input.restored_input_text = result.clone();
        self.phase = UiPhase::Idle;
        self.input_locked = false;
        self.active_cycle = false;
        self.reset_abort();
        self.ensure_invariants();
        result
    }

    pub fn request_quit_if_idle(&mut self) {
        if self.phase == UiPhase::Idle {
            self.quit_requested = true;
        }
    }

    /// Arm the Ctrl-C quit confirmation: record the arm instant and show the
    /// confirmation status message. A second Ctrl-C within
    /// [`AppState::quit_confirm_window`] quits.
    pub fn arm_quit_confirmation(&mut self) {
        self.quit_confirm_since = Some(Instant::now());
        self.status.message.set_message(CTRL_C_QUIT_CONFIRM_STATUS);
    }

    /// True only if armed AND within the confirmation window.
    pub fn quit_confirmation_armed(&self) -> bool {
        match self.quit_confirm_since {
            Some(since) => since.elapsed() < self.quit_confirm_window,
            None => false,
        }
    }

    /// Disarm the quit confirmation. The status message is cleared only while
    /// the confirmation is still within its window; an expired confirmation
    /// leaves the message to expire through its own TTL.
    ///
    /// Returns whether the status message was cleared — the only visible change
    /// this call can produce.
    pub fn disarm_quit_confirmation(&mut self) -> bool {
        let was_armed = self.quit_confirmation_armed();
        self.quit_confirm_since = None;
        if was_armed && self.status.message.status_line() == CTRL_C_QUIT_CONFIRM_STATUS {
            self.status.message.clear();
            return true;
        }
        false
    }

    /// Check inline slash suggestions based on the current textarea content.
    /// This is called from handle_insert_mode_key after textarea mutations.
    pub fn check_inline_slash(&mut self, buffer: &str) {
        let options = filter_inline_slash_suggestions(buffer);
        if options.is_empty() {
            self.picker.close();
            return;
        }
        let entry = self.picker.open(ActivePicker::InlineSlash);
        entry.state.options = options
            .into_iter()
            .map(|c| PickerOption {
                id: c.label().to_string(),
                display: c.label().to_string(),
                search_text: c.label().to_string(),
                section: None,
                sort_key: Vec::new(),
                payload: PickerPayload::Slash(c),
            })
            .collect();
    }

    pub fn ensure_invariants(&mut self) {
        let armed = self.phase == UiPhase::AbortPending && self.abort.pending;

        if self.phase == UiPhase::AbortPending && !self.active_cycle {
            self.phase = UiPhase::Idle;
            self.disarm_abort();
        }

        if self.phase != UiPhase::AbortPending {
            self.disarm_abort();
        }

        self.active_cycle = self.active_prompt_id.is_some() || !self.pending_prompt_ids.is_empty();

        if self.phase == UiPhase::Idle && self.active_cycle {
            self.phase = UiPhase::Busy;
        }

        if self.phase == UiPhase::AbortPending && !self.active_cycle {
            self.phase = UiPhase::Idle;
            self.disarm_abort();
        }

        // Both disarm blocks above drop the arm when the cycle settles. The
        // transition, not the call, is what owes the user the settled notice.
        if armed && !(self.phase == UiPhase::AbortPending && self.abort.pending) {
            self.abort.settle_notice = true;
        }

        self.picker.clamp_selections();

        self.status.mcp.clamp_mcp_panel_selection();

        // With ListState, viewport invariants are managed by ratatui automatically

        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .enforce_single_active_invariant();
    }

    /// Drop any abort arm and any settled notice owed for it. A new turn's
    /// start, a submission, and a confirmed cancel all reset the press state.
    fn reset_abort(&mut self) {
        self.disarm_abort();
        self.abort.settle_notice = false;
    }

    /// Drop the arm and its deadline, leaving the settled notice alone.
    pub(crate) fn disarm_abort(&mut self) {
        self.abort.pending = false;
        self.abort.armed_at = None;
    }
}
