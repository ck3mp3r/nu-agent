use super::*;

impl AppState {
    pub fn new() -> Self {
        Self::default()
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

    pub fn start_compaction_block(&mut self, source: &str) {
        if self
            .compaction_items
            .iter()
            .any(|item| item.source == source && item.status == CompactionStatus::InProgress)
        {
            return;
        }
        if !self.transcript_preview.is_empty() {
            let prev_is_spacer = self
                .transcript_preview
                .last()
                .is_some_and(|last| matches!(last.kind, TranscriptEntryKind::Spacer(_)));
            if !prev_is_spacer {
                self.push_spacer(); // closing spacer for previous block
            }
            self.push_spacer(); // starting spacer for compaction block
        }
        self.push_transcript_line(TranscriptRole::System, "Compaction".to_string());
        let entry_id = self.transcript_preview.last().map(|e| e.id);
        if let Some(entry) = self.transcript_preview.last_mut() {
            entry.status = Some(ItemStatus::InProgress);
        }
        self.compaction_items.push(CompactionLine {
            source: source.to_string(),
            status: CompactionStatus::InProgress,
            entry_id,
        });
    }

    pub fn finish_compaction_block(&mut self, source: &str, status: CompactionStatus) {
        let mut found_idx: Option<usize> = self
            .compaction_items
            .iter()
            .enumerate()
            .rev()
            .find(|(_, item)| item.source == source && item.status == CompactionStatus::InProgress)
            .map(|(i, _)| i);
        if found_idx.is_none() {
            found_idx = self
                .compaction_items
                .iter()
                .enumerate()
                .rev()
                .find(|(_, item)| item.status == CompactionStatus::InProgress)
                .map(|(i, _)| i);
        }
        if let Some(idx) = found_idx {
            let item = &mut self.compaction_items[idx];
            item.status = status;
            if let Some(entry_id) = item.entry_id
                && let Some(entry) = self
                    .transcript_preview
                    .iter_mut()
                    .rev()
                    .find(|e| e.id == entry_id)
            {
                entry.status = Some(match status {
                    CompactionStatus::InProgress => ItemStatus::InProgress,
                    CompactionStatus::Done => ItemStatus::Done,
                    CompactionStatus::Failed => ItemStatus::Failed,
                });
            }
        }
    }

    pub fn start_tool_call(&mut self, name: &str, arguments: &str) {
        let args_summary = nu_agent_core::protocol::tool_args::summarize_tool_arguments(arguments);
        self.push_transcript_item(TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Tool(ToolInvocation {
                name: name.to_string(),
                source: String::new(),
                args: format!("→ {args_summary}"),
            }),
            status: Some(ItemStatus::InProgress),
        });
        let entry_id = self.transcript_preview.last().map(|e| e.id);

        tool_calls::ToolCallBookkeeping::new(
            &mut self.tool_call_items,
            &mut self.active_tool_ids_by_key,
            &mut self.next_tool_call_id,
        )
        .start_tool_call(name, arguments, entry_id);
    }

    pub fn finish_tool_call(&mut self, name: &str, arguments: &str, success: bool) {
        let status = if success {
            ItemStatus::Done
        } else {
            ItemStatus::Failed
        };
        tool_calls::ToolCallBookkeeping::new(
            &mut self.tool_call_items,
            &mut self.active_tool_ids_by_key,
            &mut self.next_tool_call_id,
        )
        .finish_tool_call(
            name,
            arguments,
            success,
            &mut self.transcript_preview,
            status,
        );
    }

    pub fn accept_submit(&mut self) {
        self.phase = UiPhase::Busy;
        self.active_cycle = self.active_prompt_id.is_some() || !self.pending_prompt_ids.is_empty();
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn enqueue_external_prompt(&mut self, text: String) {
        self.push_user_block_start_spacers();
        self.push_transcript_line(TranscriptRole::User, text.clone());
        let entry_id = self.transcript_preview.last().map(|e| e.id);
        if let Some(entry) = self.transcript_preview.last_mut() {
            entry.status = Some(ItemStatus::InProgress);
        }
        self.push_spacer(); // closing spacer for user block
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
        let prev_is_spacer = self
            .transcript_preview
            .last()
            .is_some_and(|last| matches!(last.kind, TranscriptEntryKind::Spacer(_)));
        // Only push a closing spacer if there is a previous block to close.
        if !self.transcript_preview.is_empty() && !prev_is_spacer {
            self.push_spacer(); // closing spacer for previous block
        }
        self.push_spacer(); // starting spacer for user block
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
        self.push_transcript_line(TranscriptRole::User, prompt_text);
        let entry_id = self.transcript_preview.last().map(|e| e.id);
        if let Some(entry) = self.transcript_preview.last_mut() {
            entry.status = Some(ItemStatus::InProgress);
        }
        self.push_spacer(); // closing spacer for user block
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
        self.push_transcript_line(TranscriptRole::User, combined.clone());
        let entry_id = self.transcript_preview.last().map(|e| e.id);
        if let Some(entry) = self.transcript_preview.last_mut() {
            entry.status = Some(ItemStatus::InProgress);
        }
        self.push_spacer(); // closing spacer for user block
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
        let completed_id = self.active_prompt_id;
        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .complete_active_prompt();

        if let Some(id) = completed_id
            && let Some(prompt) = self.prompt_items.iter().find(|p| p.id == id)
            && let Some(entry_id) = prompt.entry_id
            && let Some(entry) = self
                .transcript_preview
                .iter_mut()
                .rev()
                .find(|e| e.id == entry_id)
        {
            entry.status = Some(ItemStatus::Done);
        }

        self.phase = UiPhase::Idle;
        self.input_locked = false;
        self.active_cycle = false;
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn cancel_active_and_pending_prompts(&mut self) {
        let cancelled_ids: Vec<u64> = self
            .prompt_items
            .iter()
            .filter(|p| p.status == PromptStatus::InProgress || p.status == PromptStatus::Queued)
            .map(|p| p.id)
            .collect();
        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .cancel_active_and_pending_prompts();

        for id in cancelled_ids {
            if let Some(prompt) = self.prompt_items.iter().find(|p| p.id == id)
                && let Some(entry_id) = prompt.entry_id
                && let Some(entry) = self
                    .transcript_preview
                    .iter_mut()
                    .rev()
                    .find(|e| e.id == entry_id)
            {
                entry.status = Some(ItemStatus::Cancelled);
            }
        }

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
    /// Also stores the result in `restored_input_text` for the coordinator
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

        self.restored_input_text = result.clone();
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

    pub fn record_token_usage(&mut self, input_tokens: u64, output_tokens: u64, total_tokens: u64) {
        self.latest_input_tokens = Some(input_tokens);
        self.latest_output_tokens = Some(output_tokens);
        self.latest_total_tokens = Some(total_tokens);
        self.session_total_tokens = self.session_total_tokens.saturating_add(total_tokens);
    }

    pub fn hydrate_latest_total_tokens(&mut self, total_tokens: u64) {
        self.latest_total_tokens = Some(total_tokens);
        if self.session_total_tokens < total_tokens {
            self.session_total_tokens = total_tokens;
        }
    }

    pub fn hydrate_usage(
        &mut self,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        total_tokens: Option<u64>,
    ) {
        if let Some(input_tokens) = input_tokens {
            self.latest_input_tokens = Some(input_tokens);
        }
        if let Some(output_tokens) = output_tokens {
            self.latest_output_tokens = Some(output_tokens);
        }
        if let Some(total_tokens) = total_tokens {
            self.hydrate_latest_total_tokens(total_tokens);
        }
    }

    pub fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.context_window_max_tokens = max_tokens;
    }

    pub fn context_window_max_tokens(&self) -> Option<u64> {
        self.context_window_max_tokens
    }

    /// Check inline slash suggestions based on the current textarea content.
    /// This is called from handle_insert_mode_key after textarea mutations.
    pub fn check_inline_slash(&mut self, buffer: &str) {
        self.inline_slash_commands = filter_inline_slash_suggestions(buffer);
        self.inline_slash_open = !self.inline_slash_commands.is_empty();
        if !self.inline_slash_open {
            self.inline_slash_selection = 0;
        } else if self.inline_slash_selection >= self.inline_slash_commands.len() {
            self.inline_slash_selection = self.inline_slash_commands.len().saturating_sub(1);
        }
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

        let palette_len = self.command_palette_actions().len();
        if palette_len == 0 {
            self.command_palette_selection = 0;
        } else if self.command_palette_selection >= palette_len {
            self.command_palette_selection = palette_len.saturating_sub(1);
        }

        if self.mcp_servers.is_empty() {
            self.mcp_panel_selection = 0;
        } else if self.mcp_panel_selection >= self.mcp_servers.len() {
            self.mcp_panel_selection = self.mcp_servers.len().saturating_sub(1);
        }

        if !self.model_picker_open {
            self.model_picker_selection = 0;
            self.model_picker_query.clear();
        } else {
            let len = self.model_picker_filtered_options().len();
            if len == 0 {
                self.model_picker_selection = 0;
            } else if self.model_picker_selection >= len {
                self.model_picker_selection = len.saturating_sub(1);
            }
        }

        if !self.agent_picker_open {
            self.agent_picker_selection = 0;
            self.agent_picker_query.clear();
        } else {
            let agent_filtered_count = self.agent_picker_filtered_options().len();
            if agent_filtered_count == 0 {
                self.agent_picker_selection = 0;
            } else if self.agent_picker_selection >= agent_filtered_count {
                self.agent_picker_selection = agent_filtered_count.saturating_sub(1);
            }
        }

        if !self.session_picker_open {
            self.session_picker_selection = 0;
            self.session_picker_query.clear();
        } else {
            let session_filtered_count = self.session_picker_filtered_options().len();
            if session_filtered_count == 0 {
                self.session_picker_selection = 0;
            } else if self.session_picker_selection >= session_filtered_count {
                self.session_picker_selection = session_filtered_count.saturating_sub(1);
            }
        }

        if !self.theme_picker_open {
            self.theme_picker_selection = 0;
        } else {
            let theme_count = self.theme_picker_options.len();
            if theme_count == 0 {
                self.theme_picker_selection = 0;
            } else if self.theme_picker_selection >= theme_count {
                self.theme_picker_selection = theme_count.saturating_sub(1);
            }
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
