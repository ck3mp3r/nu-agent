//! The pure layout planner: entries + width + gap rules → [`FramePlan`].
//!
//! The gap rules moved here from the TUI's separator state machine. They are a
//! stateless pair table: the predecessor's family and the incoming family
//! decide the row count between two neighbours. The state machine's seeding and
//! mutation collapse into the caller tracking the previous family.

use agnt_core::transcript::entry::TranscriptEntry;
use agnt_core::transcript::ir::MessageRole;

use crate::backend::{RenderBackend, RowRect};

// region:    --- Types

/// The family the gap rules classify an entry into. It is the render layer's
/// own vocabulary: the planner derives it from the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryFamily {
    User,
    Assistant,
    Tool,
    Notice,
    Banner,
}

impl EntryFamily {
    /// Classify an entry. A tool call is one `Tool` entry for its whole life,
    /// so this reads the entry variant and, for prose, the message role.
    pub fn of(entry: &TranscriptEntry) -> Self {
        match entry {
            TranscriptEntry::Message(message) => match message.role {
                MessageRole::User => Self::User,
                MessageRole::Assistant => Self::Assistant,
            },
            TranscriptEntry::Tool(_) => Self::Tool,
            TranscriptEntry::Notice(_) => Self::Notice,
            TranscriptEntry::Banner(_) => Self::Banner,
        }
    }
}

/// One entry's placement in the frame. `index` addresses the planned entry
/// slice; `gap_before` is the number of separator rows between the previous
/// entry and this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedEntry {
    pub index: usize,
    pub family: EntryFamily,
    pub gap_before: usize,
    pub rect: RowRect,
}

/// The frame the planner decides: every entry with its row rect, the gap rows
/// between them, and the trailing gap after the last entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FramePlan {
    pub entries: Vec<PlannedEntry>,
    pub trailing_gap: usize,
    pub total_rows: usize,
}

/// The pure planner. Same entries, same width, same backend measurement →
/// the same plan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutPlanner;

// endregion: --- Types

// region:    --- LayoutPlanner

impl LayoutPlanner {
    /// Separator rows between a known predecessor and the incoming family,
    /// after the components' own padding rows take over their share of the
    /// spacing.
    ///
    /// The base table is the pre-padding total — the landed pair spacing:
    /// 1. two tool blocks → 0;
    /// 2. tool → assistant → 1;
    /// 3. assistant → tool → 1;
    /// 4. every other transition → 2 (a closing row and a starting row).
    ///
    /// The planner gap is that total minus the adjacent component paddings
    /// (`prev_bottom` above, `next_top` below), saturating. The base total is
    /// therefore invariant: `gap + prev_bottom + next_top == base` whenever
    /// `base >= prev_bottom + next_top`, which holds for every pair.
    pub fn gap_between(
        prev: EntryFamily,
        incoming: EntryFamily,
        prev_bottom: usize,
        next_top: usize,
    ) -> usize {
        Self::base_gap(prev, incoming).saturating_sub(prev_bottom.saturating_add(next_top))
    }

    /// The landed four-family pair table. It is the total spacing pin: the
    /// component paddings are subtracted from it, never added to it.
    fn base_gap(prev: EntryFamily, incoming: EntryFamily) -> usize {
        if prev == EntryFamily::Tool && incoming == EntryFamily::Tool {
            return 0;
        }
        if prev == EntryFamily::Tool && incoming == EntryFamily::Assistant {
            return 1;
        }
        if prev == EntryFamily::Assistant && incoming == EntryFamily::Tool {
            return 1;
        }
        2
    }

    /// Separator rows after the last entry. A user turn relies on separators
    /// for its visual margin, so it closes with one blank row; every other
    /// family already ends flush.
    pub fn trailing_gap(last: EntryFamily) -> usize {
        match last {
            EntryFamily::User => 1,
            _ => 0,
        }
    }

    /// Plan every entry at `width`, measuring through `backend`. Rows are
    /// absolute: entry `i` starts after the accumulated entry rows and gaps.
    ///
    /// Each entry's own padding rows are part of its rect; the pair gap is the
    /// base table minus those adjacent paddings. The frame has no outer
    /// margin, so a padding row with no neighbour to space against is clipped:
    /// the first entry keeps no top padding and the last keeps no bottom
    /// padding. Every neighbouring pair therefore totals exactly the landed
    /// base spacing, and the frame's first and last rows are content rows.
    pub fn plan<B: RenderBackend>(
        entries: &[TranscriptEntry],
        width: usize,
        backend: &B,
    ) -> FramePlan {
        let width = width.max(1);
        let mut planned = Vec::with_capacity(entries.len());
        let mut row = 0;
        let last_index = entries.len().saturating_sub(1);
        let mut prev: Option<(EntryFamily, usize)> = None;

        for (index, entry) in entries.iter().enumerate() {
            let family = EntryFamily::of(entry);
            let (top, bottom) = entry.padding();
            let padding_top = if prev.is_some() { top } else { 0 };
            let padding_bottom = if index == last_index { 0 } else { bottom };
            let gap_before = prev.map_or(0, |(prev_family, prev_bottom)| {
                Self::gap_between(prev_family, family, prev_bottom, padding_top)
            });
            row += gap_before;
            let rows = backend.measure(entry, width) + padding_top + padding_bottom;
            planned.push(PlannedEntry {
                index,
                family,
                gap_before,
                rect: RowRect::new(row, width, rows).with_content_offset(padding_top),
            });
            row += rows;
            prev = Some((family, padding_bottom));
        }

        let trailing_gap = prev.map_or(0, |(family, _)| Self::trailing_gap(family));
        FramePlan {
            entries: planned,
            trailing_gap,
            total_rows: row + trailing_gap,
        }
    }
}

// endregion: --- Types

// region:    --- Tests

#[cfg(test)]
#[path = "../test/planner.rs"]
mod planner_test;

#[cfg(test)]
#[path = "../test/parity.rs"]
mod parity_test;

// endregion: --- Tests
