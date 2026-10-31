//! Compaction domain: the compaction block rows and their streamed summary.

use agnt_core::transcript::entry::{MessageEntry, NoticeEntry, TranscriptEntry};

use super::transcript_store::TranscriptStore;
use super::{CompactionLine, CompactionStatus, ScrollState, StatusState};
use agnt_core::transcript::ir::NoticeKind;

/// Compaction-domain state extracted from `AppState`: the compaction block
/// rows tracked per source.
#[derive(Debug, Clone, Default)]
pub struct CompactionState {
    pub(crate) blocks: Vec<CompactionLine>,
}

impl CompactionState {
    /// Fold a `CompactionStarted` event. Returns whether the TUI changed.
    pub fn started(&mut self, store: &mut TranscriptStore, source: &str) -> bool {
        self.start_block(store, source);
        true
    }

    /// Fold a `CompactionSummaryChunk` event: re-render the streamed summary.
    pub fn summary_chunk(
        &mut self,
        store: &mut TranscriptStore,
        scroll: &mut ScrollState,
        source: &str,
        aggregated: &str,
    ) -> bool {
        self.summary_chunk_impl(store, scroll, source, aggregated)
    }

    /// Fold a `CompactionCompleted` event: finalize the summary block.
    pub fn completed(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        source: &str,
        summary_body: &str,
    ) -> bool {
        self.completed_impl(store, status, source, summary_body)
    }

    /// Fold a `CompactionFailed` event: close the block and report the failure.
    pub fn failed(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        source: &str,
        message: &str,
    ) -> bool {
        self.failed_impl(store, status, source, message)
    }

    pub(crate) fn start_block(&mut self, store: &mut TranscriptStore, source: &str) {
        if self
            .blocks
            .iter()
            .any(|item| item.source == source && item.status == CompactionStatus::InProgress)
        {
            return;
        }
        store.push_entry(TranscriptEntry::Notice(NoticeEntry {
            kind: NoticeKind::Compaction,
            text: "Compaction".to_string(),
        }));
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

    fn summary_chunk_impl(
        &mut self,
        store: &mut TranscriptStore,
        scroll: &mut ScrollState,
        source: &str,
        text: &str,
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
        }

        // Store raw markdown — projected at render time with canvas width
        scroll.scroll_transcript_to_bottom();
        store.push_entry(TranscriptEntry::Message(MessageEntry {
            role: agnt_core::transcript::ir::MessageRole::Assistant,
            markdown: crate::markdown::unwrap_single_fenced_block(trimmed),
        }));
        true
    }

    fn completed_impl(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        source: &str,
        summary_body: &str,
    ) -> bool {
        self.start_block(store, source);
        self.finish_block(source, CompactionStatus::Done);
        let body = if summary_body.trim().is_empty() {
            "(empty summary)".to_string()
        } else {
            summary_body.to_string()
        };

        // Clear streaming state before final render pass
        if let Some(start) = store.summary_stream_start {
            store.truncate(start);
        }

        if !body.trim().is_empty() {
            store.push_entry(TranscriptEntry::Message(MessageEntry {
                role: agnt_core::transcript::ir::MessageRole::Assistant,
                markdown: crate::markdown::unwrap_single_fenced_block(&body),
            }));
        }
        store.summary_stream_start = None;
        status.message.clear();
        // Reset displayed token % — context was freed; wait for next LlmCompleted to update.
        status.tokens.latest_total_tokens = None;
        true
    }

    fn failed_impl(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        source: &str,
        message: &str,
    ) -> bool {
        self.start_block(store, source);
        self.finish_block(source, CompactionStatus::Failed);
        store.push_entry(TranscriptEntry::Notice(NoticeEntry {
            kind: NoticeKind::System,
            text: format!("Compaction failed deterministically: {message}"),
        }));
        status.message.clear();
        true
    }
}
