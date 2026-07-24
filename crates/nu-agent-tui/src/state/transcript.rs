use super::*;

const MAX_TRANSCRIPT_ENTRIES: usize = 2000;

impl AppState {
    pub fn push_transcript_line(&mut self, role: TranscriptRole, line: impl Into<String>) {
        let text = line.into();
        let entry = match role {
            TranscriptRole::User => TranscriptEntry::User(ProseMessage { markdown: text }),
            TranscriptRole::Assistant => {
                TranscriptEntry::Assistant(ProseMessage { markdown: text })
            }
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
                let entry = TranscriptEntry::Assistant(ProseMessage { markdown: text });
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

    pub(crate) fn enforce_transcript_cap(&mut self) {
        let overflow = self
            .transcript_preview
            .len()
            .saturating_sub(MAX_TRANSCRIPT_ENTRIES);
        if overflow > 0 {
            self.transcript_preview.drain(..overflow);
            self.shift_indices_after_eviction(overflow);
        }
    }

    pub(crate) fn shift_indices_after_eviction(&mut self, evicted_count: usize) {
        if evicted_count == 0 {
            return;
        }

        self.streaming_message_start = self.streaming_message_start.and_then(|n| {
            if n >= evicted_count {
                Some(n - evicted_count)
            } else {
                None
            }
        });

        self.compaction_streaming_start = self.compaction_streaming_start.and_then(|n| {
            if n >= evicted_count {
                Some(n - evicted_count)
            } else {
                None
            }
        });

        self.prompt_items = self
            .prompt_items
            .drain(..)
            .filter_map(|mut item| {
                if item.transcript_line_index >= evicted_count {
                    item.transcript_line_index -= evicted_count;
                    Some(item)
                } else {
                    None
                }
            })
            .collect();

        self.tool_call_items = self
            .tool_call_items
            .drain(..)
            .filter_map(|mut item| {
                if item.transcript_line_index >= evicted_count {
                    item.transcript_line_index -= evicted_count;
                    Some(item)
                } else {
                    None
                }
            })
            .collect();

        self.compaction_items = self
            .compaction_items
            .drain(..)
            .filter_map(|mut item| {
                if item.transcript_line_index >= evicted_count {
                    item.transcript_line_index -= evicted_count;
                    Some(item)
                } else {
                    None
                }
            })
            .collect();
    }

    pub fn push_transcript_item(&mut self, entry: TranscriptEntry) {
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
        self.enforce_transcript_cap();
    }

    pub fn scroll_transcript_line_up(&mut self) {
        self.transcript_following_tail = false;
        self.transcript_scroll_offset = self.transcript_scroll_offset.saturating_sub(1);
    }

    pub fn scroll_transcript_line_down(&mut self) {
        self.transcript_following_tail = false;
        self.transcript_scroll_offset = self.transcript_scroll_offset.saturating_add(1);
        // clamped to max_scroll at render time
    }

    pub fn scroll_transcript_page_up(&mut self, page_lines: usize) {
        self.transcript_following_tail = false;
        self.transcript_scroll_offset = self
            .transcript_scroll_offset
            .saturating_sub(page_lines.max(1));
    }

    pub fn scroll_transcript_page_down(&mut self, page_lines: usize) {
        self.transcript_following_tail = false;
        self.transcript_scroll_offset = self
            .transcript_scroll_offset
            .saturating_add(page_lines.max(1));
        // clamped to max_scroll at render time
    }

    pub fn scroll_transcript_to_top(&mut self) {
        self.transcript_following_tail = false;
        self.transcript_scroll_offset = 0;
    }

    pub fn clear_transcript(&mut self) {
        self.transcript_preview.clear();
        self.clear_assistant_projection_cache();
        self.transcript_scroll_offset = 0;
        self.transcript_following_tail = true;
        self.latest_input_tokens = None;
        self.latest_output_tokens = None;
        self.latest_total_tokens = None;
    }

    pub fn scroll_transcript_to_bottom(&mut self) {
        self.transcript_following_tail = true;
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
