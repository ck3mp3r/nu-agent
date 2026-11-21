//! Transcript store: the entry list, the streaming cursors, and the deferred
//! measure a streamed fold leaves for the frame.
//!
//! The streaming cursors ([`TranscriptStore::assistant_stream_start`] and
//! [`TranscriptStore::summary_stream_start`]) are indices into
//! [`TranscriptStore::entries`], so they live here: every entry push can
//! trigger cap eviction, and eviction shifts both cursors in one place
//! (see [`TranscriptStore::shift_indices_after_eviction`]). The LLM,
//! compaction, and turn domain reducers decide when the cursors are set,
//! truncated, and cleared; the store owns their storage and the eviction
//! bookkeeping.
//!
//! `push_entry` is the store's sole push API. The render loop reads entries
//! directly: the height index measures each entry through the TUI backend, and
//! [`TranscriptStore::frame_plan`] hands the render layer the entry rows it
//! paints. The index itself lives in [`height_index`].

// region:    --- Modules

mod height_index;
mod measure;
mod version;

use height_index::{HeightIndex, entry_metrics, pending_metrics};
use version::Versions;

pub(crate) use version::EntryVersion;

// endregion: --- Modules

use agnt_render::backend::RowRect;
use agnt_render::planner::{FramePlan, LayoutPlanner, PlannedEntry};
use agnt_render::transcript::entry::{MessageEntry, TranscriptEntry};
use agnt_render::transcript::ir::{Display, MessageRole};
use agnt_runtime::protocol::events::InferenceId;

const MAX_TRANSCRIPT_ENTRIES: usize = 2000;

/// Transcript entry storage shared by every domain reducer. Every mutation
/// keeps the height index in lockstep (measuring only the entry it touches);
/// eviction shifts the streaming cursors.
///
/// The height index agrees with the rendered output by construction because it
/// measures through the same backend the pane paints with, and that measure
/// reads no theme data: a live theme switch cannot desynchronise scroll math.
#[derive(Debug, Clone, Default)]
pub struct TranscriptStore {
    pub(crate) entries: Vec<TranscriptEntry>,
    height_index: HeightIndex,
    pub(crate) height_index_width: Option<usize>,
    pub(crate) assistant_stream_start: Option<usize>,
    /// The completion call the open streamed block belongs to. Set whenever a
    /// chunk or a start record names a call, and cleared when the block closes.
    pub(crate) assistant_stream_inference: Option<InferenceId>,
    pub(crate) summary_stream_start: Option<usize>,
    /// Lifetime count of entries dropped by the transcript cap. The reducer
    /// takes the delta across one event to shift the domain `block_index`
    /// bookkeeping, so no push site has to thread an evicted out-param.
    evicted_total: usize,
    /// The entries a fold or a push left stale: the paint path re-measures them
    /// once per frame ([`TranscriptStore::ensure_height_index`]), so a burst of
    /// chunks costs one measurement per painted frame — and none in the fold.
    stale_entries: std::collections::VecDeque<usize>,
    last_remeasure_index: Option<usize>,
    /// Lifetime count of single-entry re-measures: what the throttle saves is
    /// the difference between this count and the number of folds.
    remeasured_total: usize,
    /// Milliseconds and calls the measure path has spent re-measuring a displaced
    /// stale entry, and the diff gate has spent re-projecting the answer: the two
    /// per-arm costs that run inside a measure or a gate read, never inside a
    /// fold step. The stream arm reports their deltas.
    remeasure_millis: u128,
    remeasure_calls: u64,
    diff_gate_millis: u128,
    diff_gate_calls: u64,
    /// One cache identity per entry, parallel to `entries`: eviction drains it
    /// with the entries, so a survivor keeps the identity it was pushed with.
    versions: Versions,
    /// Painted frames that read the index so far. The streamed fold compares it
    /// to run its whole-text projection at most once per paint window.
    measure_epoch: usize,
    /// The diff gate's last answer for the open streamed block, and the epoch it
    /// was computed in. Cleared when the block is re-keyed or closed.
    diff_gate: Option<DiffGate>,
}

impl TranscriptStore {
    /// Push one entry. This is the store's sole push API. The store holds
    /// content only — separator rows are a rendering concern decided by
    /// [`agnt_render::LayoutPlanner`] at render time, not insertion time.
    /// Returns the number of entries evicted by the cap enforcement (0 when
    /// nothing was evicted) so the caller can shift its `block_index`
    /// bookkeeping by that amount.
    ///
    /// The entry's widget projection waits for the frame that paints it
    /// ([`Self::ensure_height_index`]): the fold arm that pushes a big tool
    /// result must not pay for wrapping its body.
    pub fn push_entry(&mut self, entry: TranscriptEntry) -> usize {
        self.entries.push(entry);
        self.versions.push();
        self.defer_pushed_entry();
        self.enforce_transcript_cap()
    }

    /// Key the streamed assistant block on `inference`.
    ///
    /// A key that differs from the open block's closes that block and names the
    /// new call: the text already rendered belongs to the previous call, and a
    /// later chunk of this call must not truncate it. A matching key keeps the
    /// block, so a chunk that folded ahead of its own `Started` keeps the entry
    /// it opened.
    pub(crate) fn key_assistant_stream(&mut self, inference: InferenceId) {
        if self.assistant_stream_inference != Some(inference) {
            self.assistant_stream_start = None;
            self.assistant_stream_inference = Some(inference);
            self.diff_gate = None;
        }
    }

    /// Close the streamed assistant block: the caller owns the entry it left,
    /// and the next chunk opens a new one.
    pub(crate) fn close_assistant_stream(&mut self) {
        self.assistant_stream_start = None;
        self.assistant_stream_inference = None;
        self.diff_gate = None;
    }

    /// The diff gate's answer for the open block in the current paint window.
    ///
    /// `None` means this window has not asked yet, so the caller projects the
    /// text once and records the answer with [`Self::set_diff_gate`]. A window
    /// is one painted frame: every later fold in the same frame reuses the
    /// answer, and the frame that reads the index reopens the question.
    pub(crate) fn diff_gate(&self) -> Option<bool> {
        let gate = self.diff_gate?;
        (gate.epoch == self.measure_epoch).then_some(gate.redundant)
    }

    /// Record the diff gate's answer for the current paint window.
    pub(crate) fn set_diff_gate(&mut self, redundant: bool) {
        self.diff_gate = Some(DiffGate {
            epoch: self.measure_epoch,
            redundant,
        });
    }

    /// Set the preview on the tool entry at `index` in place. The entry keeps
    /// its identity, so no companion entry is pushed; the entry joins the
    /// pending set and the frame that paints it measures it — the fold pays
    /// nothing. Returns whether the entry is a tool entry.
    pub(crate) fn set_tool_preview_at(&mut self, index: usize, preview: Display) -> bool {
        let Some(TranscriptEntry::Tool(tool)) = self.entries.get_mut(index) else {
            return false;
        };
        tool.preview = Some(preview);
        self.bump_revision(index);
        self.mark_stale(index);
        true
    }

    /// Set the completion display on the tool entry at `index` in place. The
    /// entry keeps its identity, so no companion entry is pushed; the entry
    /// joins the pending set and the frame that paints it measures it — the fold
    /// pays nothing. Returns whether the entry is a tool entry.
    pub(crate) fn set_tool_result_at(&mut self, index: usize, result: Display) -> bool {
        let Some(TranscriptEntry::Tool(tool)) = self.entries.get_mut(index) else {
            return false;
        };
        tool.result = Some(result);
        self.bump_revision(index);
        self.mark_stale(index);
        true
    }

    /// Fold one streamed markdown update into the entry at `index`.
    ///
    /// The entry keeps its identity: its markdown is replaced and its cached
    /// rows are marked stale, so a burst of chunks costs one measurement per
    /// painted frame instead of one per chunk, and no entry is pushed or evicted
    /// per chunk. When `index` holds no assistant message — the streamed cursor
    /// was lost — the tail from `index` is dropped and one fresh assistant entry
    /// is pushed, which is what the pre-fold streaming shape did on every chunk.
    pub(crate) fn fold_stream_markdown_at(&mut self, index: usize, markdown: String) {
        let slot_is_streamed = matches!(
            self.entries.get(index),
            Some(TranscriptEntry::Message(message)) if message.role == MessageRole::Assistant
        );
        if !slot_is_streamed {
            self.truncate(index);
            self.push_entry(TranscriptEntry::Message(MessageEntry {
                role: MessageRole::Assistant,
                markdown,
                timestamp_millis: None,
                model: None,
                agent: None,
            }));
            return;
        }
        if let Some(TranscriptEntry::Message(message)) = self.entries.get_mut(index) {
            message.markdown = markdown;
        }
        self.mark_stale(index);
    }

    /// Move entry `index`'s content revision, so no rows parked for its previous
    /// content are served again. The id stays, so the entry's parked canvas is
    /// replaced rather than accumulated.
    ///
    /// Every mutation site calls this — the in-place setters through
    /// [`TranscriptStore::remeasure_entry_at`], and a status stamped behind the
    /// store's back through this method directly.
    pub(crate) fn bump_revision(&mut self, index: usize) {
        self.versions.bump(index);
    }

    /// The cache identity of entry `index`: its stable id and current revision.
    pub(crate) fn entry_version(&self, index: usize) -> Option<EntryVersion> {
        self.versions.get(index)
    }

    /// Mark the height index stale — the next `rebuild_height_index` must
    /// re-measure. Called when the width changes (the resize site); in-place
    /// entry mutations re-measure through `remeasure_entry_at` instead.
    pub(crate) fn invalidate_height_index(&mut self) {
        self.height_index_width = None;
    }

    /// Whether the index is valid for `width`.
    pub(crate) fn height_index_valid_for(&self, width: usize) -> bool {
        self.height_index_width == Some(width)
            && self.height_index.entries.len() == self.entries.len()
    }

    /// Cumulative start row of entry `index` in the height index. The row
    /// includes the separator rows the entry's own span carries.
    pub(crate) fn start_row_of(&self, index: usize) -> usize {
        self.height_index.starts.get(index).copied().unwrap_or(0)
    }

    /// Total visual rows across all entries at the indexed width.
    pub(crate) fn total_visual_rows(&self) -> usize {
        self.height_index.total_rows
    }

    /// Entry index range covering visual rows `offset..offset + viewport`
    /// (exclusive end), via binary search over the height index.
    pub(crate) fn visible_window(&self, offset: usize, viewport: usize) -> (usize, usize) {
        self.height_index.visible_window(offset, viewport)
    }

    /// The frame plan for entries `first..last`: each entry's family, the gap
    /// rows before it, and its absolute row rect, from the cached
    /// measurements. `total_rows` is the whole transcript's row count — every
    /// rect is an absolute frame row, so a window's plan and the scroll math
    /// cannot disagree.
    pub(crate) fn frame_plan(&self, first: usize, last: usize, width: usize) -> FramePlan {
        let end = last.min(self.entries.len());
        let start = first.min(end);
        let mut planned = Vec::with_capacity(end - start);
        for index in start..end {
            let Some(height) = self.height_index.entries.get(index) else {
                break;
            };
            let Some(metrics) = self.height_index.metrics.get(index) else {
                break;
            };
            planned.push(PlannedEntry {
                index,
                family: metrics.family,
                gap_before: height.gap,
                rect: RowRect::new(
                    self.start_row_of(index) + height.gap,
                    width,
                    height.rows.saturating_sub(height.gap),
                )
                .with_content_offset(height.padding_top),
            });
        }
        let trailing_gap = if end == self.entries.len() {
            self.height_index
                .metrics
                .last()
                .map_or(0, |metrics| LayoutPlanner::trailing_gap(metrics.family))
        } else {
            0
        };
        FramePlan {
            entries: planned,
            trailing_gap,
            total_rows: self.height_index.total_rows,
        }
    }

    /// Enforce the transcript entry cap: drain the oldest overflow entries and
    /// shift the streaming cursors. Returns the evicted count.
    pub(crate) fn enforce_transcript_cap(&mut self) -> usize {
        let overflow = self.entries.len().saturating_sub(MAX_TRANSCRIPT_ENTRIES);
        if overflow > 0 {
            self.entries.drain(..overflow);
            self.versions.drain(overflow);
            self.evicted_total = self.evicted_total.saturating_add(overflow);
            self.shift_indices_after_eviction(overflow);
            if self.height_index_width.is_some() {
                let drained = overflow.min(self.height_index.metrics.len());
                self.height_index.metrics.drain(..drained);
                self.height_index.derive();
            }
        }
        overflow
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

        // The pending marks address entries by index, so each follows the entry
        // it names — and drops when that entry was evicted.
        self.stale_entries = self
            .stale_entries
            .drain(..)
            .filter_map(|marked| marked.checked_sub(evicted_count))
            .collect();
    }

    /// Drop every entry and end the store's lifetime: the eviction count and
    /// the streaming cursors index rows that no longer exist.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.versions.clear();
        self.height_index = HeightIndex::default();
        self.height_index_width = None;
        self.evicted_total = 0;
        self.remeasured_total = 0;
        self.measure_epoch = 0;
        self.stale_entries.clear();
        self.close_assistant_stream();
        self.summary_stream_start = None;
    }

    // region:    --- Accessors

    pub(crate) fn entries(&self) -> &[TranscriptEntry] {
        &self.entries
    }

    pub(crate) fn entries_mut(&mut self) -> &mut [TranscriptEntry] {
        &mut self.entries
    }

    /// Lifetime count of entries dropped by the transcript cap.
    pub(crate) fn evicted_total(&self) -> usize {
        self.evicted_total
    }

    /// Lifetime count of single-entry re-measures. A streamed burst of chunks
    /// leaves one measure per painted frame behind, so the growth of this count
    /// is what the deferred measure saves.
    pub(crate) fn remeasured_total(&self) -> usize {
        self.remeasured_total
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
        self.versions.truncate(len);
        self.stale_entries.retain(|&marked| marked < len);
        if self.height_index_width.is_some() {
            self.height_index.metrics.truncate(len);
            self.height_index.derive();
        }
    }

    // endregion: --- Accessors
}

// region:    --- Support

/// The diff gate's answer for the open streamed block, and the measure epoch
/// (painted frame) it was computed in.
///
/// The answer is a memo, not stored state the surface reads: it expires with the
/// frame that computed it, and [`TranscriptStore::key_assistant_stream`] and
/// [`TranscriptStore::close_assistant_stream`] drop it when the block they key
/// is re-keyed or closed. Its whole purpose is to keep a whole-text markdown
/// projection off the per-chunk fold path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiffGate {
    epoch: usize,
    redundant: bool,
}
// endregion: --- Support
