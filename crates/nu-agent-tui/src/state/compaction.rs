//! Compaction domain: compaction block rendering and the compaction-event
//! reducer.

use nu_agent_core::bus::CompactionEvent;
use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry, TranscriptEntryKind};

use super::transcript_store::TranscriptStore;
use super::{AppState, CompactionLine, CompactionStatus, ScrollState, StatusState, TranscriptRole};

/// Compaction-domain state extracted from `AppState`: the compaction block
/// rows tracked per source.
#[derive(Debug, Clone, Default)]
pub struct CompactionState {
    pub(crate) blocks: Vec<CompactionLine>,
}

impl CompactionState {
    /// Reduce a compaction lifecycle event. Returns whether the TUI changed.
    pub fn reduce_compaction_event(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        scroll: &mut ScrollState,
        event: CompactionEvent,
    ) -> bool {
        match event {
            // A compaction request is acted on by the orchestrator only; the
            // TUI does not render it.
            CompactionEvent::Requested { .. } => false,
            CompactionEvent::Started { source } => {
                self.start_block(store, &source);
                true
            }
            CompactionEvent::SummaryChunk {
                source, aggregated, ..
            } => self.summary_chunk(store, scroll, &source, aggregated),
            CompactionEvent::Completed {
                source,
                summary_preview: _,
                summary_body,
            } => self.completed(store, status, &source, summary_body),
            CompactionEvent::Failed { source, message } => {
                self.failed(store, status, &source, message)
            }
        }
    }

    pub(crate) fn start_block(&mut self, store: &mut TranscriptStore, source: &str) {
        if self
            .blocks
            .iter()
            .any(|item| item.source == source && item.status == CompactionStatus::InProgress)
        {
            return;
        }
        if !store.is_empty() {
            if !store.last_is_spacer() {
                store.push_spacer(); // closing spacer for previous block
            }
            store.push_spacer(); // starting spacer for compaction block
        }
        store.push_transcript_line(TranscriptRole::System, "Compaction".to_string());
        let entry_id = store.last_entry_id();
        store.push_spacer(); // gap between header and summary body
        self.blocks.push(CompactionLine {
            source: source.to_string(),
            status: CompactionStatus::InProgress,
            entry_id,
        });
    }

    pub(crate) fn finish_block(&mut self, source: &str, status: CompactionStatus) {
        let mut found_idx: Option<usize> = self
            .blocks
            .iter()
            .enumerate()
            .rev()
            .find(|(_, item)| item.source == source && item.status == CompactionStatus::InProgress)
            .map(|(i, _)| i);
        if found_idx.is_none() {
            found_idx = self
                .blocks
                .iter()
                .enumerate()
                .rev()
                .find(|(_, item)| item.status == CompactionStatus::InProgress)
                .map(|(i, _)| i);
        }
        if let Some(idx) = found_idx {
            let item = &mut self.blocks[idx];
            item.status = status;
        }
    }

    pub fn in_progress(&self) -> bool {
        self.blocks
            .iter()
            .any(|item| item.status == CompactionStatus::InProgress)
    }

    fn summary_chunk(
        &mut self,
        store: &mut TranscriptStore,
        scroll: &mut ScrollState,
        source: &str,
        text: String,
    ) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return false;
        }

        // Ensure compaction block is started (idempotent)
        self.start_block(store, source);

        // Track streaming start position
        if store.summary_stream_start.is_none() {
            store.summary_stream_start = Some(store.len());
        }

        // Remove previous rendering of this streaming message
        if let Some(start) = store.summary_stream_start {
            store.truncate(start);
            store.clear_assistant_projection_cache();
        }

        // Store raw markdown — projected at render time with canvas width
        scroll.scroll_transcript_to_bottom();
        store.push_transcript_item(TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Assistant(ProseMessage {
                markdown: crate::markdown::unwrap_single_fenced_block(trimmed),
            }),
            status: None,
        });
        true
    }

    fn completed(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        source: &str,
        summary_body: String,
    ) -> bool {
        self.start_block(store, source);
        self.finish_block(source, CompactionStatus::Done);
        let body = if summary_body.trim().is_empty() {
            "(empty summary)".to_string()
        } else {
            summary_body
        };

        // Clear streaming state before final render pass
        if let Some(start) = store.summary_stream_start {
            store.truncate(start);
        }

        if !body.trim().is_empty() {
            store.push_transcript_item(TranscriptEntry {
                id: 0,
                kind: TranscriptEntryKind::Assistant(ProseMessage {
                    markdown: crate::markdown::unwrap_single_fenced_block(&body),
                }),
                status: None,
            });
        }
        store.summary_stream_start = None;
        store.push_spacer();
        status.status_line.clear();
        // Reset displayed token % — context was freed; wait for next LlmCompleted to update.
        status.tokens.latest_total_tokens = None;
        true
    }

    fn failed(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        source: &str,
        message: String,
    ) -> bool {
        self.start_block(store, source);
        self.finish_block(source, CompactionStatus::Failed);
        store.push_transcript_line(
            TranscriptRole::System,
            format!("Compaction failed deterministically: {message}"),
        );
        status.status_line.clear();
        true
    }
}

/// Single dispatch seam for the compaction domain: owns the
/// (`CompactionState`, `TranscriptStore`, `StatusState`, `ScrollState`)
/// borrow split so both event paths share it.
pub(crate) fn dispatch_compaction_event(state: &mut AppState, event: CompactionEvent) -> bool {
    state.compaction.reduce_compaction_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.scroll,
        event,
    )
}
