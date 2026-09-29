//! reducer.

use nu_agent_core::bus::CompactionEvent;
use nu_agent_core::transcript::ir::Block;
use nu_agent_core::transcript::items::Message;
use nu_agent_core::transcript::renderer::Renderable;

use super::transcript_store::TranscriptStore;
use super::{AppState, CompactionLine, CompactionStatus, ScrollState, StatusState};
use nu_agent_core::transcript::ir::NoticeKind;
use nu_agent_core::transcript::items::Notice;

/// Compaction-domain state extracted from `AppState`: the compaction block
/// rows tracked per source.
#[derive(Debug, Clone, Default)]
pub struct CompactionState {
    pub(crate) blocks: Vec<CompactionLine>,
}

impl CompactionState {
    /// Reduce a compaction lifecycle event. Returns whether the TUI changed.
    /// The evicted count from any block push inside is returned via `evicted`
    /// so the top-level caller can shift domain bookkeeping.
    pub fn reduce_compaction_event(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        scroll: &mut ScrollState,
        event: CompactionEvent,
        evicted: &mut usize,
    ) -> bool {
        match event {
            // A compaction request is acted on by the orchestrator only; the
            // TUI does not render it.
            CompactionEvent::Requested { .. } => false,
            CompactionEvent::Started { source } => {
                self.start_block(store, &source, evicted);
                true
            }
            CompactionEvent::SummaryChunk {
                source, aggregated, ..
            } => self.summary_chunk(store, scroll, &source, aggregated, evicted),
            CompactionEvent::Completed {
                source,
                summary_preview: _,
                summary_body,
            } => self.completed(store, status, &source, summary_body, evicted),
            CompactionEvent::Failed { source, message } => {
                self.failed(store, status, &source, message, evicted)
            }
        }
    }

    pub(crate) fn start_block(
        &mut self,
        store: &mut TranscriptStore,
        source: &str,
        evicted: &mut usize,
    ) {
        if self
            .blocks
            .iter()
            .any(|item| item.source == source && item.status == CompactionStatus::InProgress)
        {
            return;
        }
        let header = Notice {
            kind: NoticeKind::Compaction,
            text: "Compaction".to_string(),
        };
        *evicted += store.push_block(Block {
            source: header.source(),
            lane: header.lane(),
            fill: header.fill(),
            status: None,
        });
        self.blocks.push(CompactionLine {
            source: source.to_string(),
            status: CompactionStatus::InProgress,
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
        evicted: &mut usize,
    ) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return false;
        }

        // Ensure compaction block is started (idempotent)
        self.start_block(store, source, evicted);

        // Track streaming start position
        if store.summary_stream_start.is_none() {
            store.summary_stream_start = Some(store.len());
        }

        // Remove previous rendering of this streaming message
        if let Some(start) = store.summary_stream_start {
            store.truncate(start);
        }

        // Store raw markdown — projected at render time with canvas width
        scroll.scroll_transcript_to_bottom();
        let msg = Message {
            role: nu_agent_core::transcript::ir::MessageRole::Assistant,
            markdown: crate::markdown::unwrap_single_fenced_block(trimmed),
        };
        *evicted += store.push_block(Block {
            source: msg.source(),
            lane: msg.lane(),
            fill: msg.fill(),
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
        evicted: &mut usize,
    ) -> bool {
        self.start_block(store, source, evicted);
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
            let msg = Message {
                role: nu_agent_core::transcript::ir::MessageRole::Assistant,
                markdown: crate::markdown::unwrap_single_fenced_block(&body),
            };
            *evicted += store.push_block(Block {
                source: msg.source(),
                lane: msg.lane(),
                fill: msg.fill(),
                status: None,
            });
        }
        store.summary_stream_start = None;
        status.message.clear();
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
        evicted: &mut usize,
    ) -> bool {
        self.start_block(store, source, evicted);
        self.finish_block(source, CompactionStatus::Failed);
        let failure = Notice {
            kind: NoticeKind::System,
            text: format!("Compaction failed deterministically: {message}"),
        };
        *evicted += store.push_block(Block {
            source: failure.source(),
            lane: failure.lane(),
            fill: failure.fill(),
            status: None,
        });
        status.message.clear();
        true
    }
}

/// Single dispatch seam for the compaction domain: owns the
/// (`CompactionState`, `TranscriptStore`, `ScrollState`)
/// borrow split so both event paths share it. Any eviction caused by the
/// event's pushes shifts the domain block_index bookkeeping.
pub(crate) fn dispatch_compaction_event(state: &mut AppState, event: CompactionEvent) -> bool {
    let mut evicted = 0usize;
    let changed = state.compaction.reduce_compaction_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.scroll,
        event,
        &mut evicted,
    );
    state.shift_bookkeeping_after_eviction(evicted);
    changed
}
