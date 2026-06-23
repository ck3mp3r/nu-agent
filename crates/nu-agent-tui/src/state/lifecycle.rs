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

    #[cfg(test)]
    pub fn active_prompt_id(&self) -> Option<u64> {
        self.active_prompt_id
    }

    #[cfg(test)]
    pub fn pending_prompt_ids(&self) -> &VecDeque<u64> {
        &self.pending_prompt_ids
    }

    pub fn pending_prompt_count(&self) -> usize {
        self.pending_prompt_ids.len()
    }

    pub fn prompt_status_for_transcript_line(
        &self,
        transcript_line_index: usize,
    ) -> Option<PromptStatus> {
        self.prompt_items
            .iter()
            .rev()
            .find(|prompt| {
                if prompt.transcript_line_index == usize::MAX {
                    return false;
                }
                prompt.transcript_line_index == transcript_line_index
            })
            .map(|prompt| prompt.status)
    }

    pub fn transcript_line_status_for_index(
        &self,
        transcript_line_index: usize,
    ) -> Option<TranscriptLineStatus> {
        if let Some(status) = self.prompt_status_for_transcript_line(transcript_line_index) {
            return Some(TranscriptLineStatus::Prompt(status));
        }

        if let Some(status) = self
            .compaction_items
            .iter()
            .rev()
            .find(|item| item.transcript_line_index == transcript_line_index)
            .map(|item| item.status)
        {
            return Some(TranscriptLineStatus::Compaction(status));
        }

        self.tool_call_items
            .iter()
            .rev()
            .find(|tool| tool.transcript_line_index == transcript_line_index)
            .map(|tool| TranscriptLineStatus::Tool(tool.status))
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
            self.push_transcript_line(TranscriptRole::Separator, String::new());
        }
        self.push_transcript_line(TranscriptRole::System, "Compaction".to_string());
        let transcript_line_index = self.transcript_preview.len().saturating_sub(1);
        self.compaction_items.push(CompactionLine {
            transcript_line_index,
            source: source.to_string(),
            status: CompactionStatus::InProgress,
        });
    }

    pub fn finish_compaction_block(&mut self, source: &str, status: CompactionStatus) {
        if let Some(item) = self
            .compaction_items
            .iter_mut()
            .rev()
            .find(|item| item.source == source && item.status == CompactionStatus::InProgress)
        {
            item.status = status;
            return;
        }

        if let Some(item) = self
            .compaction_items
            .iter_mut()
            .rev()
            .find(|item| item.status == CompactionStatus::InProgress)
        {
            item.status = status;
        }
    }

    pub fn start_tool_call(&mut self, name: &str, arguments: &str) {
        let args_summary = nu_agent_core::protocol::tool_args::summarize_tool_arguments(arguments);
        let line_text = format!("tool[{name}] args={args_summary}");
        self.push_transcript_line(TranscriptRole::Tool, line_text);

        let transcript_line_index = self.transcript_preview.len().saturating_sub(1);
        tool_calls::ToolCallBookkeeping::new(
            &mut self.tool_call_items,
            &mut self.active_tool_ids_by_key,
            &mut self.next_tool_call_id,
        )
        .start_tool_call(transcript_line_index, name, arguments);
    }

    pub fn finish_tool_call(&mut self, name: &str, arguments: &str, success: bool) {
        tool_calls::ToolCallBookkeeping::new(
            &mut self.tool_call_items,
            &mut self.active_tool_ids_by_key,
            &mut self.next_tool_call_id,
        )
        .finish_tool_call(name, arguments, success);
    }

    pub fn accept_submit(&mut self) {
        self.input.buffer.clear();
        self.input.cursor = 0;
        self.phase = UiPhase::Busy;
        self.active_cycle = self.active_prompt_id.is_some() || !self.pending_prompt_ids.is_empty();
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn enqueue_external_prompt(&mut self, text: String) {
        self.push_transcript_line(TranscriptRole::User, text.clone());
        let transcript_line_index = self.transcript_preview.len().saturating_sub(1);
        let id = self.next_prompt_id;
        self.next_prompt_id = self.next_prompt_id.saturating_add(1);
        self.prompt_items.push(QueuedPrompt {
            id,
            prompt_text: text,
            transcript_line_index,
            status: PromptStatus::InProgress,
        });
        self.active_prompt_id = Some(id);
        self.phase = UiPhase::Busy;
        self.active_cycle = true;
    }

    pub fn enqueue_prompt(&mut self, submitted_text: String) -> u64 {
        let id = prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .enqueue_prompt(submitted_text, usize::MAX);
        self.accept_submit();
        id
    }

    pub fn enqueue_immediate_submission(&mut self, submitted_text: String) {
        self.pending_immediate_submissions.push_back(submitted_text);
        self.input.buffer.clear();
        self.input.cursor = 0;
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
        self.push_transcript_line(TranscriptRole::User, prompt_text);
        let real_index = self.transcript_preview.len().saturating_sub(1);
        if let Some(prompt) = self.prompt_items.iter_mut().find(|p| p.id == active_id) {
            prompt.transcript_line_index = real_index;
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

        let active_id = self.activate_next_prompt()?;
        self.prompt_items
            .iter()
            .find(|prompt| prompt.id == active_id)
            .map(|prompt| prompt.prompt_text.clone())
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

    pub fn request_quit_if_idle(&mut self) {
        if self.phase == UiPhase::Idle && self.input.buffer.is_empty() {
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

    pub fn ensure_invariants(&mut self) {
        if self.phase == UiPhase::AbortPending && !self.active_cycle {
            self.phase = UiPhase::Idle;
            self.abort.pending = false;
        }

        if self.phase != UiPhase::AbortPending {
            self.abort.pending = false;
        }

        self.active_cycle = self.active_prompt_id.is_some() || !self.pending_prompt_ids.is_empty();

        self.inline_slash_commands = filter_inline_slash_suggestions(&self.input.buffer);
        self.inline_slash_open = !self.inline_slash_commands.is_empty();
        if !self.inline_slash_open {
            self.inline_slash_selection = 0;
        } else if self.inline_slash_selection >= self.inline_slash_commands.len() {
            self.inline_slash_selection = self.inline_slash_commands.len().saturating_sub(1);
        }

        if self.phase == UiPhase::Idle && self.active_cycle {
            self.phase = UiPhase::Busy;
        }

        if self.phase == UiPhase::AbortPending && !self.active_cycle {
            self.phase = UiPhase::Idle;
            self.abort.pending = false;
        }

        if self.input.cursor > self.input.buffer.len() {
            self.input.cursor = self.input.buffer.len();
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

        while self.input.cursor > 0 && !self.input.buffer.is_char_boundary(self.input.cursor) {
            self.input.cursor -= 1;
        }

        // With ListState, viewport invariants are managed by ratatui automatically

        self.input.locked = false;

        prompt_queue::PromptQueueLifecycle::new(
            &mut self.prompt_items,
            &mut self.pending_prompt_ids,
            &mut self.active_prompt_id,
            &mut self.next_prompt_id,
        )
        .enforce_single_active_invariant();
    }
}
