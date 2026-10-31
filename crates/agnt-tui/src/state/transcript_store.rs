//! Transcript domain store: the entry list, the streaming cursors, and the
//! measure()-based height index.
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
//! paints.

use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::Display;
use agnt_render::RenderBackend;
use agnt_render::backend::RowRect;
use agnt_render::planner::{EntryFamily, FramePlan, LayoutPlanner, PlannedEntry};

use crate::widgets::RatatuiBackend;

const MAX_TRANSCRIPT_ENTRIES: usize = 2000;

/// One entry's rows in the frame: the separator rows before it and the total
/// rows it spans at the indexed width.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HeightEntry {
    /// Separator rows between the previous entry and this one.
    gap: usize,
    /// Total rows this entry spans at the indexed width: `gap` plus the rows
    /// its own content renders to plus the padding rows the frame keeps inside
    /// its rect.
    rows: usize,
    /// Realized top padding: blank rows between the rect's first row and the
    /// entry's first content row.
    padding_top: usize,
}

/// Cached per-entry inputs to the gap walk: the measured content rows, the
/// entry family, and the entry's own padding. Derived from the entry's data
/// once per measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntryMetrics {
    content_rows: usize,
    family: EntryFamily,
    padding: (usize, usize),
}

/// Cumulative row offsets over the entry list, for O(log n) viewport-window
/// queries. Maintained incrementally: `metrics` caches each entry's measured
/// row count at the indexed width, and [`HeightIndex::derive`] walks the entry
/// list through the planner's gap rules to refresh the offsets. A mutation
/// measures only the entry it touches — never the whole list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HeightIndex {
    /// Cumulative start row for entry i is `starts[i]`; entry i spans
    /// `starts[i]..starts[i] + entries[i].rows`.
    starts: Vec<usize>,
    entries: Vec<HeightEntry>,
    total_rows: usize,
    /// Per-entry measurement inputs at the indexed width, parallel to the
    /// entry list. Holds content rows only — separator rows depend on the
    /// neighbour pair, so [`HeightIndex::derive`] re-applies them on every
    /// walk.
    metrics: Vec<EntryMetrics>,
}

impl HeightIndex {
    /// Wholesale rebuild: measure every entry at `width`, then derive the
    /// offsets from the fresh cache.
    ///
    /// Measurement goes through the backend the render loop paints with, so a
    /// measured row count and a painted row count cannot drift apart. The
    /// backend's measure reads no theme data by construction, so any theme
    /// yields the same row counts.
    fn rebuild(entries: &[TranscriptEntry], width: usize) -> Self {
        let mut index = Self {
            metrics: entries
                .iter()
                .map(|entry| entry_metrics(entry, width))
                .collect(),
            ..Self::default()
        };
        index.derive();
        index
    }

    /// Refresh `starts`, `entries`, and `total_rows` from the cached entry
    /// metrics by walking the entry list through the planner's gap rules.
    /// O(n) enum-match walk — no markdown projection, no measurement.
    ///
    /// Separator rows are a rendering concern (see
    /// [`agnt_render::LayoutPlanner`]); the store holds content entries only.
    /// Each entry's span therefore includes the separator rows the renderer
    /// emits *before* it and the padding rows its own rect carries, so
    /// `total_visual_rows` and `visible_window` stay in lockstep with the
    /// rendered output. The leading separator rows belong to the incoming
    /// entry's span, matching the render loop's emission order. After the last
    /// entry, the trailing separator rows (a user turn closes with one) are
    /// added to `total_rows` so the total still matches the rendered output
    /// (task 670e0292).
    ///
    /// Padding is clipped at the frame edges exactly as the planner clips it:
    /// the first entry keeps no top padding and the last keeps no bottom
    /// padding, so no outer margin appears and every neighbouring pair totals
    /// the landed base spacing.
    fn derive(&mut self) {
        self.starts.clear();
        self.entries.clear();
        self.total_rows = 0;
        let last_index = self.metrics.len().saturating_sub(1);
        let mut prev: Option<(EntryFamily, usize)> = None;
        for (index, metrics) in self.metrics.iter().enumerate() {
            self.starts.push(self.total_rows);
            let (top, bottom) = metrics.padding;
            let padding_top = if prev.is_some() { top } else { 0 };
            let padding_bottom = if index == last_index { 0 } else { bottom };
            let separators = prev.map_or(0, |(prev_family, prev_bottom)| {
                LayoutPlanner::gap_between(prev_family, metrics.family, prev_bottom, padding_top)
            });
            let rows = separators + metrics.content_rows + padding_top + padding_bottom;
            self.entries.push(HeightEntry {
                gap: separators,
                rows,
                padding_top,
            });
            self.total_rows += rows;
            prev = Some((metrics.family, padding_bottom));
        }
        self.total_rows += prev.map_or(0, |(family, _)| LayoutPlanner::trailing_gap(family));
    }

    /// Binary-search the entry index range covering visual rows
    /// `offset..offset + viewport_height`. Returns `(first, last)` as an
    /// exclusive-end range; both are 0 when nothing is visible.
    fn visible_window(&self, offset: usize, viewport_height: usize) -> (usize, usize) {
        if self.entries.is_empty() || viewport_height == 0 {
            return (0, 0);
        }
        let end_row = offset.saturating_add(viewport_height);
        // First entry whose end row extends past `offset` — earlier entries
        // end at or above the viewport top and are scrolled out.
        let first = self
            .starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        // First entry that starts at or past `end_row` — it and everything
        // after are below the viewport.
        let last = self.starts.partition_point(|&start| start < end_row);
        (first, last)
    }
}

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
    pub(crate) height_index: HeightIndex,
    pub(crate) height_index_width: Option<usize>,
    pub(crate) assistant_stream_start: Option<usize>,
    pub(crate) summary_stream_start: Option<usize>,
    /// Lifetime count of entries dropped by the transcript cap. The reducer
    /// takes the delta across one event to shift the domain `block_index`
    /// bookkeeping, so no push site has to thread an evicted out-param.
    evicted_total: usize,
}

impl TranscriptStore {
    /// Push one entry. This is the store's sole push API. The store holds
    /// content only — separator rows are a rendering concern decided by
    /// [`agnt_render::LayoutPlanner`] at render time, not insertion time.
    /// Returns the number of entries evicted by the cap enforcement (0 when
    /// nothing was evicted) so the caller can shift its `block_index`
    /// bookkeeping by that amount.
    pub fn push_entry(&mut self, entry: TranscriptEntry) -> usize {
        self.entries.push(entry);
        self.cache_pushed_entry();
        self.enforce_transcript_cap()
    }

    /// Set the preview on the tool entry at `index` in place. The entry keeps
    /// its identity, so no companion entry is pushed. Returns whether the
    /// entry is a tool entry.
    pub(crate) fn set_tool_preview_at(&mut self, index: usize, preview: Display) -> bool {
        let Some(TranscriptEntry::Tool(tool)) = self.entries.get_mut(index) else {
            return false;
        };
        tool.preview = Some(preview);
        self.invalidate_height_index();
        true
    }

    /// Set the completion display on the tool entry at `index` in place. The
    /// entry keeps its identity, so no companion entry is pushed; the in-place
    /// mutation invalidates the cached rows, exactly as the preview path does.
    /// Returns whether the entry is a tool entry.
    pub(crate) fn set_tool_result_at(&mut self, index: usize, result: Display) -> bool {
        let Some(TranscriptEntry::Tool(tool)) = self.entries.get_mut(index) else {
            return false;
        };
        tool.result = Some(result);
        self.invalidate_height_index();
        true
    }

    /// Measure the entry just pushed, append its metrics to the cache, then
    /// re-derive the offsets. No-op when the index is stale — the next
    /// `rebuild_height_index` re-measures everything.
    fn cache_pushed_entry(&mut self) {
        let Some(width) = self.height_index_width else {
            return;
        };
        let Some(entry) = self.entries.last() else {
            return;
        };
        self.height_index.metrics.push(entry_metrics(entry, width));
        self.height_index.derive();
    }

    /// Rebuild the height index from the backend's `measure(entry, width)` for
    /// every entry.
    pub(crate) fn rebuild_height_index(&mut self, width: usize) {
        self.height_index = HeightIndex::rebuild(&self.entries, width);
        self.height_index_width = Some(width);
    }

    /// Mark the height index stale — the next `rebuild_height_index` must
    /// re-measure. Called when the width changes or an entry mutates in place
    /// (preview) or the list changes outside `push_entry`/`truncate` (resize).
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
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.height_index = HeightIndex::default();
        self.height_index_width = None;
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

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
        if self.height_index_width.is_some() {
            self.height_index.metrics.truncate(len);
            self.height_index.derive();
        }
    }

    // endregion: --- Accessors
}

// region:    --- Support

/// Measure one entry through the backend the render loop paints with, and
/// cache the separator-walk inputs.
fn entry_metrics(entry: &TranscriptEntry, width: usize) -> EntryMetrics {
    let probe = RatatuiBackend::new(u16::try_from(width).unwrap_or(u16::MAX), 0);
    EntryMetrics {
        content_rows: probe.measure(entry, width),
        family: EntryFamily::of(entry),
        padding: entry.padding(),
    }
}

// endregion: --- Support
