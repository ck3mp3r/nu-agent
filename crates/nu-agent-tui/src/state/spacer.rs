//! Peer-aware separator decisions for the transcript render layer.
//!
//! Separators are a *rendering* concern: blank rows that visually separate
//! two adjacent blocks. The store holds content blocks only; the renderer
//! walks the block list and asks [`SpacerStateMachine`] how many blank rows
//! belong between each pair. The state machine sees one block at a time
//! (`BlockFamily` + `has_filled`) and never inspects a `BlockSource`.

use nu_agent_core::transcript::ir::BlockFamily;

/// Decides the number of blank separator rows before an incoming block from
/// the previously-seen block. Streaming: feed each block in order with
/// [`SpacerStateMachine::separators_for`].
///
/// Transition rules:
/// 1. first block (no predecessor) → 0;
/// 2. two tool-family blocks (Tool or ToolDisplay) → 0, except a filled
///    ToolDisplay followed by a Tool → 1;
/// 3. tool family → assistant → 1;
/// 4. assistant → tool family → 1;
/// 5. every other transition → 2 (a closing row and a starting row).
#[derive(Debug, Clone, Default)]
pub(crate) struct SpacerStateMachine {
    prev_family: Option<BlockFamily>,
    prev_has_filled: bool,
}

impl SpacerStateMachine {
    /// Seed the machine from an already-seen predecessor. Used when a render
    /// slice starts mid-transcript: the first visible block's separator count
    /// depends on the block just above it, not on `None`.
    pub(crate) fn seeded(prev_family: BlockFamily, prev_has_filled: bool) -> Self {
        Self {
            prev_family: Some(prev_family),
            prev_has_filled,
        }
    }

    /// Number of separator rows to emit before the incoming block, then
    /// advance the machine to make the incoming block the new predecessor.
    pub(crate) fn separators_for(
        &mut self,
        incoming: BlockFamily,
        incoming_has_filled: bool,
    ) -> usize {
        let count = match self.prev_family {
            None => 0,
            Some(prev) => Self::count_between(prev, self.prev_has_filled, incoming),
        };
        self.prev_family = Some(incoming);
        self.prev_has_filled = incoming_has_filled;
        count
    }

    /// Number of separator rows to emit after the last block. A user turn
    /// relies on separators for its visual margin (`Fill::Full` has no margin
    /// rows), so it closes with one blank row; every other family already ends
    /// flush and needs none. The machine remembers the last block it saw.
    pub(crate) fn trailing_separators(&self) -> usize {
        match self.prev_family {
            Some(BlockFamily::User) => 1,
            _ => 0,
        }
    }

    /// Separator count between a known predecessor and the incoming family.
    fn count_between(prev: BlockFamily, prev_has_filled: bool, incoming: BlockFamily) -> usize {
        let prev_is_tool = prev.is_tool_family();
        let incoming_is_tool = incoming.is_tool_family();

        if prev_is_tool && incoming_is_tool {
            // A filled display is a background region; a following tool call
            // needs one row of separation so the two blocks read apart.
            let filled_display_then_tool = prev == BlockFamily::ToolDisplay
                && prev_has_filled
                && incoming == BlockFamily::Tool;
            return usize::from(filled_display_then_tool);
        }
        if prev_is_tool && incoming == BlockFamily::Assistant {
            return 1;
        }
        if prev == BlockFamily::Assistant && incoming_is_tool {
            return 1;
        }
        2
    }
}

// region:    --- Tests

#[cfg(test)]
#[path = "spacer_test.rs"]
mod tests;

// endregion: --- Tests
