//! Transcript domain store: the transcript entry list, the streaming
//! cursors, and the assistant markdown projection cache.
//!
//! The streaming cursors ([`TranscriptStore::assistant_stream_start`] and
//! [`TranscriptStore::summary_stream_start`]) are indices into
//! [`TranscriptStore::entries`], so they live here: every entry push can
//! trigger cap eviction, and eviction shifts both cursors in one place
//! (see [`TranscriptStore::shift_indices_after_eviction`]). The LLM,
//! compaction, and turn domain reducers decide when the cursors are set,
//! truncated, and cleared; the store owns their storage and the eviction
//! bookkeeping.

use std::collections::HashMap;

use nu_agent_core::protocol::contracts::UiMessageSnapshot;
use nu_agent_core::transcript::ir::{ContentLine, DisplayLine, Role};
use nu_agent_core::transcript::items::{
    ProseMessage, Spacer as SpacerItem, SystemMessage, ToolInvocation,
    ToolResult as TranscriptToolResult, TranscriptEntry, TranscriptEntryKind, annotate_diff_hint,
};

use super::{
    CompactionState, CompactionStatus, EntryVisualInfo, ScrollState, StatusState, ToolState,
    TranscriptRole,
};
use crate::state::tool_parsing::{extract_tool_name, parse_persisted_tool_status_line};

const MAX_TRANSCRIPT_ENTRIES: usize = 2000;

/// Transcript entry storage shared by every domain reducer. Every entry push
/// marks the visual-info cache dirty; eviction shifts the streaming cursors.
#[derive(Debug, Clone)]
pub struct TranscriptStore {
    pub(crate) entries: Vec<TranscriptEntry>,
    pub(crate) assistant_projection_cache: HashMap<String, Vec<ContentLine>>,
    pub(crate) visual_info_dirty: bool,
    pub(crate) assistant_stream_start: Option<usize>,
    pub(crate) summary_stream_start: Option<usize>,
    next_entry_id: u64,
}

impl Default for TranscriptStore {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            assistant_projection_cache: HashMap::new(),
            visual_info_dirty: true,
            assistant_stream_start: None,
            summary_stream_start: None,
            next_entry_id: 1,
        }
    }
}

impl TranscriptStore {
    pub fn push_transcript_line(&mut self, role: TranscriptRole, line: impl Into<String>) {
        let text = line.into();
        let entry = match role {
            TranscriptRole::User => TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::User(ProseMessage { markdown: text }),
                status: None,
            },
            TranscriptRole::Assistant => TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::Assistant(ProseMessage { markdown: text }),
                status: None,
            },
            TranscriptRole::Tool => {
                let args = text.trim_start_matches("→ ").to_string();
                TranscriptEntry {
                    id: 0,
                    kind: TranscriptEntryKind::Tool(ToolInvocation {
                        name: String::new(),
                        source: String::new(),
                        args,
                    }),
                    status: None,
                }
            }
            TranscriptRole::ToolDisplay => TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::ToolResult(TranscriptToolResult {
                    name: String::new(),
                    success: true,
                    lines: vec![DisplayLine::new(text.clone(), annotate_diff_hint(&text))],
                }),
                status: None,
            },
            TranscriptRole::Compaction => TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::System(SystemMessage { text }),
                status: None,
            },
            TranscriptRole::System => TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::System(SystemMessage { text }),
                status: None,
            },
        };
        self.push_transcript_item(entry);
    }

    pub fn project_assistant_markdown_lines(&mut self, markdown: &str) -> Vec<ContentLine> {
        crate::markdown::render_markdown_lines(markdown, None)
    }

    pub fn clear_assistant_projection_cache(&mut self) {
        self.assistant_projection_cache.clear();
    }

    pub(crate) fn assistant_projection_cache_mut(
        &mut self,
    ) -> &mut HashMap<String, Vec<ContentLine>> {
        &mut self.assistant_projection_cache
    }

    pub(crate) fn enforce_transcript_cap(&mut self) {
        let overflow = self.entries.len().saturating_sub(MAX_TRANSCRIPT_ENTRIES);
        if overflow > 0 {
            self.entries.drain(..overflow);
            self.shift_indices_after_eviction(overflow);
            self.visual_info_dirty = true;
        }
    }

    pub(crate) fn shift_indices_after_eviction(&mut self, evicted_count: usize) {
        if evicted_count == 0 {
            return;
        }

        self.assistant_stream_start = self.assistant_stream_start.and_then(|n| {
            if n >= evicted_count {
                Some(n - evicted_count)
            } else {
                None
            }
        });

        self.summary_stream_start = self.summary_stream_start.and_then(|n| {
            if n >= evicted_count {
                Some(n - evicted_count)
            } else {
                None
            }
        });
    }

    pub fn push_transcript_item(&mut self, mut entry: TranscriptEntry) {
        entry.id = self.next_entry_id;
        self.next_entry_id = self.next_entry_id.saturating_add(1);
        self.entries.push(entry);
        self.visual_info_dirty = true;
        self.enforce_transcript_cap();
    }

    /// Push a spacer (empty line) unconditionally.
    /// Used to explicitly start and close transcript blocks.
    /// Two adjacent blocks have two spacers between them (closing + starting).
    pub fn push_spacer(&mut self) {
        self.entries.push(TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Spacer(SpacerItem),
            status: None,
        });
        self.visual_info_dirty = true;
        self.enforce_transcript_cap();
    }

    pub(crate) fn recompute_entry_visual_info(&mut self, scroll: &mut ScrollState, width: usize) {
        use nu_agent_core::transcript::items::Renderable;

        let mut info = Vec::with_capacity(self.entries.len());
        let mut start = 0usize;
        for entry in &self.entries {
            let block = entry.to_render_block();
            let content_lines: Vec<ContentLine> = if let Some(md) = &block.markdown {
                if let Some(cached) = self.assistant_projection_cache.get(md) {
                    cached.clone()
                } else {
                    let projected = crate::markdown::render_markdown_lines(md, Some(width as u16));
                    self.assistant_projection_cache
                        .insert(md.clone(), projected.clone());
                    projected
                }
            } else {
                block.lines
            };
            let prefix_width = crate::tui_renderer::lane_prefix_width();
            let effective_width = width.saturating_sub(prefix_width).max(1);
            let visual_rows: usize = content_lines
                .iter()
                .map(|line| {
                    let ratatui_line = ratatui::text::Line::from(
                        line.spans
                            .iter()
                            .map(|s| ratatui::text::Span::raw(s.text.clone()))
                            .collect::<Vec<_>>(),
                    );
                    ratatui::widgets::Paragraph::new(ratatui_line)
                        .wrap(ratatui::widgets::Wrap::default())
                        .line_count(effective_width as u16)
                        .max(1)
                })
                .sum::<usize>()
                .max(1);
            info.push(EntryVisualInfo {
                start_visual_row: start,
                visual_row_count: visual_rows,
            });
            start += visual_rows;
        }
        scroll.entry_visual_info = info;
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.clear_assistant_projection_cache();
    }

    // region:    --- Accessors

    pub(crate) fn entries(&self) -> &[TranscriptEntry] {
        &self.entries
    }

    pub(crate) fn entries_mut(&mut self) -> &mut [TranscriptEntry] {
        &mut self.entries
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn last(&self) -> Option<&TranscriptEntry> {
        self.entries.last()
    }

    pub(crate) fn last_entry_id(&self) -> Option<u64> {
        self.entries.last().map(|e| e.id)
    }

    pub(crate) fn last_is_spacer(&self) -> bool {
        self.entries
            .last()
            .is_some_and(|last| matches!(last.kind, TranscriptEntryKind::Spacer(_)))
    }

    /// Role of the last non-spacer entry, if any.
    pub(crate) fn last_content_role(&self) -> Option<Role> {
        self.entries
            .iter()
            .rev()
            .find(|e| !matches!(e.kind, TranscriptEntryKind::Spacer(_)))
            .map(|e| e.role())
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
    }

    // endregion: --- Accessors

    // region:    --- Hydration

    pub(crate) fn hydrate_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
        status: &mut StatusState,
        tool: &mut ToolState,
        compaction: &mut CompactionState,
    ) {
        for mut message in messages {
            if let Some(usage) = message.usage() {
                status.hydrate_usage(
                    usage.input_tokens(),
                    usage.output_tokens(),
                    usage.total_tokens(),
                );
            }
            if let Some(display) = message.take_tool_display() {
                crate::state::append_direct_tool_display(self, display);
                continue;
            }
            let role = match message.role() {
                "user" => TranscriptRole::User,
                "assistant" => TranscriptRole::Assistant,
                "tool" => TranscriptRole::Tool,
                "compaction" => TranscriptRole::Compaction,
                _ => TranscriptRole::System,
            };
            let message_content = message.content();

            if role == TranscriptRole::Compaction {
                compaction.start_block(self, "history");
                compaction.finish_block("history", CompactionStatus::Done);

                if !message_content.trim().is_empty() {
                    self.push_transcript_item(TranscriptEntry {
                        id: 0,
                        kind: TranscriptEntryKind::Assistant(ProseMessage {
                            markdown: crate::markdown::unwrap_single_fenced_block(message_content),
                        }),
                        status: None,
                    });
                }
                self.push_spacer();
                continue;
            }

            if message_content.trim().is_empty() {
                continue;
            }
            if role == TranscriptRole::Assistant {
                self.push_hydrate_block_start_spacers(role);
                self.push_transcript_item(TranscriptEntry {
                    id: 0,
                    kind: TranscriptEntryKind::Assistant(ProseMessage {
                        markdown: message_content.trim().to_string(),
                    }),
                    status: None,
                });
                self.push_spacer();
                continue;
            }

            if role == TranscriptRole::Tool {
                let persisted = message_content.trim();
                if let Some(arguments) = message.tool_arguments() {
                    let success = message.tool_success().unwrap_or(true);
                    let name = message
                        .tool_name()
                        .unwrap_or_else(|| extract_tool_name(persisted));
                    self.push_hydrate_tool_block_start_spacers();
                    tool.start_tool_call(self, name, arguments);
                    tool.finish_tool_call(self, name, arguments, success);
                    continue;
                }
                if let Some((name, arguments, success)) =
                    parse_persisted_tool_status_line(persisted)
                {
                    self.push_hydrate_tool_block_start_spacers();
                    tool.start_tool_call(self, name, arguments);
                    tool.finish_tool_call(self, name, arguments, success);
                    continue;
                }
                continue;
            }

            self.push_hydrate_block_start_spacers(role);
            for line in message_content.lines() {
                if !line.trim().is_empty() {
                    self.push_transcript_line(role, line.to_string());
                }
            }
            self.push_spacer();
        }

        if self.hydrate_tool_block_is_open() {
            self.push_spacer();
        }

        if let Some(tokens) = last_total_tokens {
            status.hydrate_latest_total_tokens(tokens);
        }
    }

    fn push_hydrate_block_start_spacers(&mut self, role: TranscriptRole) {
        let last_content = self.last_content_role();
        let prev_is_tool_block = matches!(last_content, Some(Role::Tool) | Some(Role::ToolDisplay));

        if role == TranscriptRole::Assistant && prev_is_tool_block {
            self.push_spacer();
            return;
        }

        let prev_is_spacer = self.last_is_spacer();
        if !self.entries.is_empty() && !prev_is_spacer {
            self.push_spacer();
        }
        self.push_spacer();
    }

    fn push_hydrate_tool_block_start_spacers(&mut self) {
        if self.hydrate_tool_block_is_open() {
            return;
        }
        let last_content = self.last_content_role();
        let prev_is_assistant = matches!(last_content, Some(Role::Assistant));

        if prev_is_assistant {
            if !self.last_is_spacer() {
                self.push_spacer();
            }
            return;
        }

        self.push_hydrate_block_start_spacers(TranscriptRole::Tool);
    }

    fn hydrate_tool_block_is_open(&self) -> bool {
        self.entries.last().is_some_and(|last| {
            matches!(
                last.kind,
                TranscriptEntryKind::Tool(_) | TranscriptEntryKind::ToolResult(_)
            )
        })
    }

    // endregion: --- Hydration
}
