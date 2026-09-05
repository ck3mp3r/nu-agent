use nu_agent_core::protocol::slash::filter_inline_slash_suggestions;

use super::*;

impl AppState {
    pub fn new_with_sender(
        event_tx: tokio::sync::mpsc::Sender<nu_agent_core::orchestrator::OrchestratorEvent>,
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
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn enqueue_external_prompt(&mut self, text: String) {
        self.push_user_block_start_spacers();
        self.transcript
            .push_transcript_line(TranscriptRole::User, text.clone());
        let entry_id = self.transcript.last_entry_id();
        self.transcript.push_spacer(); // closing spacer for user block
        let id = self.next_prompt_id;
        self.next_prompt_id = self.next_prompt_id.saturating_add(1);
        self.prompt_items.push(QueuedPrompt {
            id,
            prompt_text: text,
            status: PromptStatus::InProgress,
            entry_id,
        });
        self.active_prompt_id = Some(id);
        self.phase = UiPhase::Busy;
        self.active_cycle = true;
    }

    /// Push the closing spacer for the previous block (if not already a Spacer)
    /// followed by the starting spacer for a new user block. Two adjacent blocks
    /// get two spacers between them (closing + starting).
    fn push_user_block_start_spacers(&mut self) {
        let prev_is_spacer = self.transcript.last_is_spacer();
        // Only push a closing spacer if there is a previous block to close.
        if !self.transcript.is_empty() && !prev_is_spacer {
            self.transcript.push_spacer(); // closing spacer for previous block
        }
        self.transcript.push_spacer(); // starting spacer for user block
    }

    pub fn enqueue_prompt(&mut self, submitted_text: String) -> u64 {
        let id = prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .enqueue_prompt(submitted_text);
        self.accept_submit();
        id
    }

    pub fn enqueue_immediate_submission(&mut self, submitted_text: String) {
        self.pending_immediate_submissions.push_back(submitted_text);
        self.abort.pending = false;
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
        let prompt_text = self
            .prompt_items
            .iter()
            .find(|p| p.id == active_id)
            .map(|p| p.prompt_text.clone())
            .unwrap_or_default();
        self.push_user_block_start_spacers();
        self.transcript
            .push_transcript_line(TranscriptRole::User, prompt_text);
        let entry_id = self.transcript.last_entry_id();
        self.transcript.push_spacer(); // closing spacer for user block
        if let Some(prompt) = self.prompt_items.iter_mut().find(|p| p.id == active_id) {
            prompt.entry_id = entry_id;
        }

        self.phase = UiPhase::Busy;
        self.active_cycle = true;
        self.abort.pending = false;
        self.ensure_invariants();
        Some(active_id)
    }

    pub fn take_next_prompt_for_execution(&mut self) -> Option<String> {
        if let Some(immediate) = self.pending_immediate_submissions.pop_front() {
            self.ensure_invariants();
            return Some(immediate);
        }

        let texts = prompt_queue::PromptQueueLifecycle::new(
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

        let combined = texts.join("\n\n");
        self.push_user_block_start_spacers();
        self.transcript
            .push_transcript_line(TranscriptRole::User, combined.clone());
        let entry_id = self.transcript.last_entry_id();
        self.transcript.push_spacer(); // closing spacer for user block
        if let Some(active_id) = self.active_prompt_id
            && let Some(prompt) = self.prompt_items.iter_mut().find(|p| p.id == active_id)
        {
            prompt.entry_id = entry_id;
        }

        self.phase = UiPhase::Busy;
        self.active_cycle = true;
        self.abort.pending = false;
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

        self.phase = UiPhase::Idle;
        self.input_locked = false;
        self.active_cycle = false;
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn cancel_active_and_pending_prompts(&mut self) {
        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .cancel_active_and_pending_prompts();

        self.phase = UiPhase::Idle;
        self.input_locked = false;
        self.active_cycle = false;
        self.abort.pending = false;
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
        self.abort.confirmation_marker = self.abort.confirmation_marker.saturating_add(1);
        self.phase = UiPhase::AbortPending;
        self.ensure_invariants();
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
        self.abort.pending = false;
        self.ensure_invariants();
        result
    }

    pub fn request_quit_if_idle(&mut self) {
        if self.phase == UiPhase::Idle {
            self.quit_requested = true;
        }
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
                payload: PickerPayload::Slash(c),
            })
            .collect();
    }

    pub fn ensure_invariants(&mut self) {
        if self.phase == UiPhase::AbortPending && !self.active_cycle {
            self.phase = UiPhase::Idle;
            self.abort.pending = false;
        }

        if self.phase != UiPhase::AbortPending {
            self.abort.pending = false;
        }

        self.active_cycle = self.active_prompt_id.is_some() || !self.pending_prompt_ids.is_empty();

        if self.phase == UiPhase::Idle && self.active_cycle {
            self.phase = UiPhase::Busy;
        }

        if self.phase == UiPhase::AbortPending && !self.active_cycle {
            self.phase = UiPhase::Idle;
            self.abort.pending = false;
        }

        self.picker.clamp_selections();

        if self.status.mcp.mcp_servers.is_empty() {
            self.status.mcp.mcp_panel_selection = 0;
        } else if self.status.mcp.mcp_panel_selection >= self.status.mcp.mcp_servers.len() {
            self.status.mcp.mcp_panel_selection =
                self.status.mcp.mcp_servers.len().saturating_sub(1);
        }

        // With ListState, viewport invariants are managed by ratatui automatically

        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .enforce_single_active_invariant();
    }
}
