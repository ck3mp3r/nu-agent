use super::*;

impl AppState {
    pub fn push_transcript_line(&mut self, role: TranscriptRole, line: impl Into<String>) {
        let text = line.into();
        let entry = match role {
            TranscriptRole::User => TranscriptEntry::User(ProseMessage {
                markdown: text,
            }),
            TranscriptRole::Assistant => TranscriptEntry::Assistant(ProseMessage {
                markdown: text,
            }),
            TranscriptRole::Tool => {
                let (name, args) = parse_tool_text(&text);
                TranscriptEntry::Tool(ToolInvocation {
                    name,
                    source: String::new(),
                    args,
                })
            }
            TranscriptRole::ToolDisplay => TranscriptEntry::ToolResult(TranscriptToolResult {
                name: String::new(),
                success: true,
                lines: vec![DisplayLine::new(text.clone(), annotate_diff_hint(&text))],
            }),
            TranscriptRole::Compaction => TranscriptEntry::System(SystemMessage { text }),
            TranscriptRole::System => TranscriptEntry::System(SystemMessage { text }),
            TranscriptRole::Separator => TranscriptEntry::Separator(TranscriptSeparator),
        };
        self.push_transcript_item(entry);
    }

    pub fn push_transcript_rendered_line(&mut self, role: TranscriptRole, line: Line<'static>) {
        match role {
            TranscriptRole::Assistant | TranscriptRole::Compaction => {
                let text = rendered_line_to_plain_text(&line);
                let entry = TranscriptEntry::Assistant(ProseMessage {
                    markdown: text,
                });
                self.push_transcript_item(entry);
            }
            _ => {
                let text = rendered_line_to_plain_text(&line);
                self.push_transcript_line(role, text);
            }
        }
    }

    pub fn project_assistant_markdown_lines(&mut self, markdown: &str) -> Vec<Line<'static>> {
        if let Some(cached) = self.assistant_projection_cache.get(markdown) {
            return cached.clone();
        }

        let projected = project_markdown_to_lines(markdown, None);
        self.assistant_projection_cache
            .insert(markdown.to_string(), projected.clone());
        #[cfg(test)]
        {
            self.assistant_projection_cache_misses =
                self.assistant_projection_cache_misses.saturating_add(1);
        }
        projected
    }

    pub fn clear_assistant_projection_cache(&mut self) {
        self.assistant_projection_cache.clear();
    }

    #[cfg(test)]
    pub fn assistant_projection_cache_size(&self) -> usize {
        self.assistant_projection_cache.len()
    }

    #[cfg(test)]
    pub fn assistant_projection_cache_misses(&self) -> usize {
        self.assistant_projection_cache_misses
    }

    pub fn push_transcript_item(&mut self, entry: TranscriptEntry) {
        // Check if we should follow tail (user is at end, or nothing selected)
        let was_at_end = match self.transcript_list_state.selected {
            Some(idx) => idx + 1 >= self.transcript_preview.len(),
            None => true,
        };

        let entry_role = entry.role();
        if should_insert_turn_separator(
            self.transcript_preview.last().map(|e| e.role()).as_ref(),
            &entry_role,
        ) {
            self.transcript_preview
                .push(TranscriptEntry::Separator(TranscriptSeparator));
        }

        // Visual spacer between different roles (checks previous role AFTER separator may have been inserted)
        if needs_spacer(
            self.transcript_preview.last().map(|e| e.role()).as_ref(),
            &entry_role,
        ) {
            self.transcript_preview
                .push(TranscriptEntry::Spacer(SpacerItem));
        }

        self.transcript_preview.push(entry);

        // Only follow tail if user was already at the end
        if was_at_end {
            self.transcript_list_state
                .select(Some(self.transcript_preview.len().saturating_sub(1)));
        }
    }

    pub fn scroll_transcript_page_up(&mut self, page_lines: usize) {
        let current = self.transcript_list_state.selected.unwrap_or(0);
        self.transcript_list_state
            .select(Some(current.saturating_sub(page_lines.max(1))));
    }

    pub fn scroll_transcript_line_up(&mut self) {
        let current = self.transcript_list_state.selected.unwrap_or(0);
        self.transcript_list_state
            .select(Some(current.saturating_sub(1)));
    }

    pub fn scroll_transcript_page_down(&mut self, page_lines: usize) {
        let current = self.transcript_list_state.selected.unwrap_or(0);
        let last = self.transcript_preview.len().saturating_sub(1);
        self.transcript_list_state
            .select(Some(current.saturating_add(page_lines.max(1)).min(last)));
    }

    pub fn scroll_transcript_line_down(&mut self) {
        let current = self.transcript_list_state.selected.unwrap_or(0);
        let last = self.transcript_preview.len().saturating_sub(1);
        self.transcript_list_state
            .select(Some(current.saturating_add(1).min(last)));
    }

    pub fn scroll_transcript_to_top(&mut self) {
        self.transcript_list_state.select(Some(0));
    }

    pub fn scroll_transcript_to_bottom(&mut self) {
        let last = self.transcript_preview.len().saturating_sub(1);
        self.transcript_list_state.select(Some(last));
    }

    pub fn focus_prev_pane(&mut self) {
        self.pane_focus = match self.pane_focus {
            PaneFocus::Transcript => PaneFocus::Input,
            PaneFocus::Input => PaneFocus::Transcript,
        };
    }

    pub fn focus_next_pane(&mut self) {
        self.pane_focus = match self.pane_focus {
            PaneFocus::Transcript => PaneFocus::Input,
            PaneFocus::Input => PaneFocus::Transcript,
        };
    }
}

fn should_insert_turn_separator(previous: Option<&Role>, next: &Role) -> bool {
    matches!(
        (previous, next),
        (Some(prev), next) if is_turn_role(prev) && is_turn_role(next) && prev != next
    )
}

fn is_turn_role(role: &Role) -> bool {
    matches!(role, Role::User | Role::Assistant | Role::Tool)
}

pub(crate) fn needs_spacer(previous: Option<&Role>, next: &Role) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if previous == next {
        return false;
    }
    if *previous == Role::Separator || *next == Role::Separator {
        return false;
    }
    !matches!(
        (previous, next),
        (Role::User, Role::Assistant)
            | (Role::Assistant, Role::User)
            | (Role::Tool, Role::ToolDisplay)
            | (Role::ToolDisplay, Role::Tool)
    )
}


