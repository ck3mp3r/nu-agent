//! Compaction domain: the compaction block rows and their streamed summary.

use agnt_core::transcript::entry::{MessageEntry, NoticeEntry, TaskEntry, TranscriptEntry};

use super::transcript_store::TranscriptStore;
use super::{CompactionLine, CompactionStatus, ScrollState, StatusState};
use agnt_core::transcript::ir::NoticeKind;
use agnt_core::transcript::renderer::ItemStatus;

/// The name the compaction task's call line carries.
const COMPACTION_TASK: &str = "Compaction";

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
        self.ensure_block(source);
        self.ensure_task(store, source, ItemStatus::InProgress);
    }

    /// Record the source's in-progress block once.
    fn ensure_block(&mut self, source: &str) {
        if self
            .blocks
            .iter()
            .any(|item| item.source == source && item.status == CompactionStatus::InProgress)
        {
            return;
        }
        self.blocks.push(CompactionLine {
            source: source.to_string(),
            status: CompactionStatus::InProgress,
        });
    }

    /// Push the compaction task's call line when the transcript holds none for
    /// `source`, opening at `status`.
    ///
    /// A replayed completion (`source` `history`) opens the line already `Done`:
    /// the event that started the compaction is not replayed, so opening it
    /// `InProgress` would flash a spinner the session never showed.
    fn ensure_task(&mut self, store: &mut TranscriptStore, source: &str, status: ItemStatus) {
        let present = store.entries().iter().any(|entry| match entry {
            TranscriptEntry::Task(task) => {
                task.summary == source && task.status == ItemStatus::InProgress
            }
            _ => false,
        });
        if present {
            return;
        }
        store.push_entry(TranscriptEntry::Task(TaskEntry {
            name: COMPACTION_TASK.to_string(),
            summary: source.to_string(),
            status,
        }));
    }

    /// Move the compaction task's call line to `status` in place.
    ///
    /// The line is found by scanning the store, never by index: a cap eviction
    /// between start and finish shifts every index, and the scan still holds
    /// the row. The entry keeps its identity, so the line is never re-pushed
    /// and its text is never rewritten.
    pub(crate) fn finish_block(
        &mut self,
        store: &mut TranscriptStore,
        source: &str,
        status: CompactionStatus,
    ) {
        let mut mutated_row: Option<usize> = None;
        for (index, entry) in store.entries_mut().iter_mut().enumerate().rev() {
            let TranscriptEntry::Task(task) = entry else {
                continue;
            };
            if task.summary != source || task.status != ItemStatus::InProgress {
                continue;
            }
            task.status = item_status(status);
            mutated_row = Some(index);
            break;
        }
        if let Some(index) = mutated_row {
            store.remeasure_entry_at(index);
        }
        if let Some(item) = self
            .blocks
            .iter_mut()
            .rev()
            .find(|item| item.source == source && item.status == CompactionStatus::InProgress)
        {
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
            timestamp_millis: None,
            model: None,
            agent: None,
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
        self.ensure_block(source);
        self.ensure_task(store, source, ItemStatus::Done);
        self.finish_block(store, source, CompactionStatus::Done);
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
                timestamp_millis: None,
                model: None,
                agent: None,
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
        self.finish_block(store, source, CompactionStatus::Failed);
        store.push_entry(TranscriptEntry::Notice(NoticeEntry {
            kind: NoticeKind::System,
            text: format!("Compaction failed deterministically: {message}"),
        }));
        status.message.clear();
        true
    }
}

// region:    --- Support

/// The call line's status for the block's bookkeeping status.
fn item_status(status: CompactionStatus) -> ItemStatus {
    match status {
        CompactionStatus::InProgress => ItemStatus::InProgress,
        CompactionStatus::Done => ItemStatus::Done,
        CompactionStatus::Failed => ItemStatus::Failed,
    }
}

// endregion:    --- Support
