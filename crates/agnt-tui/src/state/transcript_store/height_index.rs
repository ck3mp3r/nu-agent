//! The height index: one measured row count per entry, and the cumulative row
//! offsets the scroll math and the frame plan read.
//!
//! The store keeps the index in lockstep with its entries: a push measures the
//! entry it appends, an in-place mutation measures the entry it touches, and a
//! width change rebuilds the whole index. Nothing here knows about pushes or
//! cursors — it measures entries and walks the planner's gap rules.

use agnt_render::RenderBackend;
use agnt_render::planner::{EntryFamily, FramePlan, LayoutPlanner};
use agnt_render::transcript::entry::TranscriptEntry;

use crate::widgets::RatatuiBackend;

/// One entry's rows in the frame: the separator rows before it and the total
/// rows it spans at the indexed width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HeightEntry {
    /// Separator rows between the previous entry and this one.
    pub(super) gap: usize,
    /// Total rows this entry spans at the indexed width: `gap` plus the rows
    /// its own content renders to plus the padding rows the frame keeps inside
    /// its rect.
    pub(super) rows: usize,
    /// Realized top padding: blank rows between the rect's first row and the
    /// entry's first content row.
    pub(super) padding_top: usize,
}

/// Cached per-entry inputs to the gap walk: the measured content rows, the
/// entry family, and the entry's own padding. Derived from the entry's data
/// once per measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EntryMetrics {
    pub(super) content_rows: usize,
    pub(super) family: EntryFamily,
    pub(super) padding: (usize, usize),
}

/// Cumulative row offsets over the entry list, for O(log n) viewport-window
/// queries. Maintained incrementally: `metrics` caches each entry's measured
/// row count at the indexed width, and [`HeightIndex::derive`] walks the entry
/// list through the planner's gap rules to refresh the offsets. A mutation
/// measures only the entry it touches — never the whole list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct HeightIndex {
    /// Cumulative start row for entry i is `starts[i]`; entry i spans
    /// `starts[i]..starts[i] + entries[i].rows`.
    pub(super) starts: Vec<usize>,
    pub(super) entries: Vec<HeightEntry>,
    pub(super) total_rows: usize,
    /// Per-entry measurement inputs at the indexed width, parallel to the
    /// entry list. Holds content rows only — separator rows depend on the
    /// neighbour pair, so [`HeightIndex::derive`] re-applies them on every
    /// walk.
    pub(super) metrics: Vec<EntryMetrics>,
}

impl HeightIndex {
    /// Wholesale rebuild: delegate the whole layout to
    /// [`LayoutPlanner::plan`] at `width`, then convert the plan into the index.
    ///
    /// The planner decides the same gap rules the incremental walk re-applies,
    /// so a full recompute and an incremental update cannot disagree on a row.
    /// Measurement goes through the backend the render loop paints with, so a
    /// measured row count and a painted row count cannot drift apart — and the
    /// backend's measure reads no theme data by construction, so any theme
    /// yields the same row counts.
    ///
    /// The plan measures each entry exactly once through its stateless backend
    /// parameter, so this path costs one measurement per entry.
    pub(super) fn rebuild(entries: &[TranscriptEntry], width: usize) -> Self {
        let probe = RatatuiBackend::new(u16::try_from(width).unwrap_or(u16::MAX), 0);
        let plan = LayoutPlanner::plan(entries, width, &probe);
        Self::from_plan(&plan, entries)
    }

    /// Convert a [`FramePlan`] into the index.
    ///
    /// The plan's rects carry absolute frame rows, so an entry's start row is
    /// its rect's row minus the gap the plan put before it, and the entry's
    /// span is that gap plus the rect's own rows.
    ///
    /// `content_rows` recovers the measured content by subtracting the padding
    /// rows the frame clips from the rect: the first entry keeps no top padding
    /// and the last keeps no bottom padding, exactly as the incremental walk
    /// clips them.
    fn from_plan(plan: &FramePlan, entries: &[TranscriptEntry]) -> Self {
        let last_index = plan.entries.len().saturating_sub(1);
        let mut starts = Vec::with_capacity(plan.entries.len());
        let mut indexed = Vec::with_capacity(plan.entries.len());
        let mut metrics = Vec::with_capacity(plan.entries.len());
        for (position, planned) in plan.entries.iter().enumerate() {
            let Some(entry) = entries.get(planned.index) else {
                break;
            };
            let padding = entry.padding();
            let padding_bottom = if position == last_index { 0 } else { padding.1 };
            starts.push(planned.rect.row.saturating_sub(planned.gap_before));
            indexed.push(HeightEntry {
                gap: planned.gap_before,
                rows: planned.gap_before + planned.rect.rows,
                padding_top: planned.rect.content_offset,
            });
            metrics.push(EntryMetrics {
                content_rows: planned
                    .rect
                    .rows
                    .saturating_sub(planned.rect.content_offset)
                    .saturating_sub(padding_bottom),
                family: planned.family,
                padding,
            });
        }
        Self {
            starts,
            entries: indexed,
            total_rows: plan.total_rows,
            metrics,
        }
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
    ///
    /// A pending placeholder ([`pending_metrics`]) carries the entry's real
    /// family and padding but zero content rows, so the entry's own rows
    /// under-count for the frame that has not measured it yet: the separators
    /// around it are already correct, only its content span waits.
    pub(super) fn derive(&mut self) {
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
    pub(super) fn visible_window(&self, offset: usize, viewport_height: usize) -> (usize, usize) {
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

// region:    --- Support

/// The placeholder metrics for an entry the frame has not measured yet.
///
/// The family and the padding are the entry's own data, so the gap walk keeps
/// the real entry shape; only the content rows wait for the frame — and those
/// are the widget projection a big tool result pays for. [`HeightIndex::derive`]
/// therefore counts the entry's content as zero rows until the frame measures
/// it.
///
/// This is the cheap half of [`entry_metrics`]: it reads the entry, it never
/// projects it.
pub(super) fn pending_metrics(entry: &TranscriptEntry) -> EntryMetrics {
    EntryMetrics {
        content_rows: 0,
        family: EntryFamily::of(entry),
        padding: entry.padding(),
    }
}

/// Measure one entry through the backend the render loop paints with, and
/// cache the separator-walk inputs.
pub(super) fn entry_metrics(entry: &TranscriptEntry, width: usize) -> EntryMetrics {
    let probe = RatatuiBackend::new(u16::try_from(width).unwrap_or(u16::MAX), 0);
    EntryMetrics {
        content_rows: probe.measure(entry, width),
        family: EntryFamily::of(entry),
        padding: entry.padding(),
    }
}

// endregion: --- Support
