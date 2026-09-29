//! Transcript domain store: the block list, the streaming cursors, the
//! measure()-based height index, and hydration.
//!
//! The streaming cursors ([`TranscriptStore::assistant_stream_start`] and
//! [`TranscriptStore::summary_stream_start`]) are indices into
//! [`TranscriptStore::blocks`], so they live here: every block push can
//! trigger cap eviction, and eviction shifts both cursors in one place
//! (see [`TranscriptStore::shift_indices_after_eviction`]). The LLM,
//! compaction, and turn domain reducers decide when the cursors are set,
//! truncated, and cleared; the store owns their storage and the eviction
//! bookkeeping.

use nu_agent_core::protocol::contracts::UiMessageSnapshot;
use nu_agent_core::transcript::ir::{Block, BlockSource, Fill, MessageRole, NoticeKind, ToolName};
use nu_agent_core::transcript::items::{Message, Notice, Tool};
use nu_agent_core::transcript::renderer::{ItemStatus, Renderable};

use super::{CompactionState, CompactionStatus, StatusState, ToolState};
use crate::state::tool_parsing::{extract_tool_name, parse_persisted_tool_status_line};

const MAX_TRANSCRIPT_BLOCKS: usize = 2000;

/// Per-block row count, rebuilt from `measure(block, width)`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HeightEntry {
    /// Number of visual rows the block renders to at the indexed width.
    rows: usize,
}

/// Cumulative row offsets over the block list, for O(log n) viewport-window
/// queries. Rebuilt wholesale from `measure(block, width)` — no incremental
/// bookkeeping to keep in lockstep with pushes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HeightIndex {
    /// Cumulative start row for block i is `starts[i]`; block i spans
    /// `starts[i]..starts[i] + entries[i].rows`.
    starts: Vec<usize>,
    entries: Vec<HeightEntry>,
    total_rows: usize,
}

impl HeightIndex {
    /// Rebuild the index by measuring every block at `width`.
    ///
    /// Separator rows are a rendering concern (see
    /// [`crate::state::spacer::SpacerStateMachine`]); the store holds content
    /// blocks only. Each block's row count therefore includes the separator
    /// rows the renderer emits *before* it, so `total_visual_rows` and
    /// `visible_window` stay in lockstep with the rendered output. The leading
    /// separator rows belong to the incoming block's span, matching the render
    /// loop's emission order. After the last block, the trailing separator rows
    /// (a user turn closes with one) are added to `total_rows` so the total
    /// still matches the rendered output (task 670e0292).
    fn rebuild(blocks: &[Block], width: usize) -> Self {
        let mut starts = Vec::with_capacity(blocks.len());
        let mut entries = Vec::with_capacity(blocks.len());
        let mut total = 0usize;
        let mut sm = crate::state::spacer::SpacerStateMachine::default();
        for block in blocks {
            starts.push(total);
            let separators = sm.separators_for(block.source.family(), block.has_filled_content());
            let rows = separators + crate::tui_renderer::measure(block, width);
            entries.push(HeightEntry { rows });
            total += rows;
        }
        // Trailing separator rows after the final block (task 670e0292).
        total += sm.trailing_separators();
        Self {
            starts,
            entries,
            total_rows: total,
        }
    }

    /// Binary-search the block index range covering visual rows
    /// `offset..offset + viewport_height`. Returns `(first, last)` as an
    /// exclusive-end range; both are 0 when nothing is visible.
    fn visible_window(&self, offset: usize, viewport_height: usize) -> (usize, usize) {
        if self.entries.is_empty() || viewport_height == 0 {
            return (0, 0);
        }
        let end_row = offset.saturating_add(viewport_height);
        // First block whose end row extends past `offset` — earlier blocks
        // end at or above the viewport top and are scrolled out.
        let first = self
            .starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        // First block that starts at or past `end_row` — it and everything
        // after are below the viewport.
        let last = self.starts.partition_point(|&start| start < end_row);
        (first, last)
    }
}

/// Transcript block storage shared by every domain reducer. Every push marks
/// the height index dirty; eviction shifts the streaming cursors.
#[derive(Debug, Clone, Default)]
pub struct TranscriptStore {
    pub(crate) blocks: Vec<Block>,
    pub(crate) height_index: HeightIndex,
    pub(crate) height_index_width: Option<usize>,
    pub(crate) assistant_stream_start: Option<usize>,
    pub(crate) summary_stream_start: Option<usize>,
}

impl TranscriptStore {
    /// Push a content block. The store holds content only — separator rows
    /// are a rendering concern decided by
    /// [`crate::state::spacer::SpacerStateMachine`] at render time, not
    /// insertion time. Returns the number of blocks evicted by the cap
    /// enforcement (0 when nothing was evicted) so the caller can shift its
    /// `block_index` bookkeeping by that amount.
    pub fn push_block(&mut self, block: Block) -> usize {
        self.blocks.push(block);
        self.height_index_width = None;
        self.enforce_transcript_cap()
    }

    /// Insert a content block at `index`, shifting every later block up by
    /// one. Used when a block must land adjacent to an existing one (a tool
    /// preview block directly after its Tool block) rather than at the tail.
    /// Returns the number of blocks evicted by the cap enforcement (0 when
    /// nothing was evicted) so the caller can shift its `block_index`
    /// bookkeeping by that amount.
    pub fn insert_block_at(&mut self, index: usize, block: Block) -> usize {
        let index = index.min(self.blocks.len());
        self.blocks.insert(index, block);
        self.height_index_width = None;
        self.enforce_transcript_cap()
    }

    /// Rebuild the height index from `measure(block, width)` for every block.
    pub(crate) fn rebuild_height_index(&mut self, width: usize) {
        self.height_index = HeightIndex::rebuild(&self.blocks, width);
        self.height_index_width = Some(width);
    }

    /// Mark the height index stale — the next `rebuild_height_index` must
    /// re-measure. Called when the width changes or content mutates outside
    /// `push_block`/`truncate` (theme switch, resize).
    pub(crate) fn invalidate_height_index(&mut self) {
        self.height_index_width = None;
    }

    /// Whether the index is valid for `width`.
    pub(crate) fn height_index_valid_for(&self, width: usize) -> bool {
        self.height_index_width == Some(width)
            && self.height_index.entries.len() == self.blocks.len()
    }

    /// Cumulative start row of block `index` in the height index.
    pub(crate) fn start_row_of(&self, index: usize) -> usize {
        self.height_index.starts.get(index).copied().unwrap_or(0)
    }

    /// Total visual rows across all blocks at the indexed width.
    pub(crate) fn total_visual_rows(&self) -> usize {
        self.height_index.total_rows
    }

    /// Block index range covering visual rows `offset..offset + viewport`
    /// (exclusive end), via binary search over the height index.
    pub(crate) fn visible_window(&self, offset: usize, viewport: usize) -> (usize, usize) {
        self.height_index.visible_window(offset, viewport)
    }

    /// Enforce the transcript block cap: drain the oldest overflow blocks and
    /// shift the streaming cursors. Returns the evicted count.
    pub(crate) fn enforce_transcript_cap(&mut self) -> usize {
        let overflow = self.blocks.len().saturating_sub(MAX_TRANSCRIPT_BLOCKS);
        if overflow > 0 {
            self.blocks.drain(..overflow);
            self.shift_indices_after_eviction(overflow);
            self.height_index_width = None;
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
        self.blocks.clear();
        self.height_index = HeightIndex::default();
        self.height_index_width = None;
    }

    // region:    --- Accessors

    pub(crate) fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub(crate) fn blocks_mut(&mut self) -> &mut [Block] {
        &mut self.blocks
    }

    pub(crate) fn len(&self) -> usize {
        self.blocks.len()
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.blocks.truncate(len);
        self.height_index_width = None;
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
        for message in messages {
            if let Some(usage) = message.usage() {
                status.tokens.hydrate_usage(
                    usage.input_tokens(),
                    usage.output_tokens(),
                    usage.total_tokens(),
                );
            }
            // The resolver emits a persisted result display as a STANDALONE
            // "tool_display" snapshot (empty content, no tool fields) that
            // follows the tool call's own snapshot; attach it to the most
            // recent Tool block as its preview. Read immutably so the
            // content borrow below stays live.
            let preview_display = message.tool_display().cloned();
            let message_role = match message.role() {
                "user" => Some(MessageRole::User),
                "assistant" => Some(MessageRole::Assistant),
                _ => None,
            };
            let message_content = message.content();

            if message.role() == "compaction" {
                let mut hydration_evicted = 0usize;
                compaction.start_block(self, "history", &mut hydration_evicted);
                compaction.finish_block("history", CompactionStatus::Done);

                if !message_content.trim().is_empty() {
                    let msg = Message {
                        role: MessageRole::Assistant,
                        markdown: crate::markdown::unwrap_single_fenced_block(message_content),
                    };
                    // Hydration rebuilds domain state from scratch; the
                    // evicted count carries no information for it.
                    let _ = self.push_block(Block {
                        source: msg.source(),
                        lane: msg.lane(),
                        fill: msg.fill(),
                        status: None,
                    });
                }
                continue;
            }

            // A standalone "tool_display" snapshot (empty content, no tool
            // fields) hydrates as its own ToolDisplay block directly after the
            // most recent Tool block — the same two-block shape the live
            // preview path produces. The empty-content guard below would
            // otherwise drop it.
            if message.role() == "tool_display" {
                if let Some(display) = preview_display {
                    let display = crate::state::tool::preview_to_display(&display);
                    // The preceding Tool block owns the tool identity, so the
                    // hydrated preview gets the same redundant-row suppression
                    // the live preview path applies.
                    let tool_index = self
                        .blocks
                        .iter()
                        .rposition(|block| matches!(block.source, BlockSource::Tool { .. }));
                    let name = tool_index
                        .and_then(|index| self.blocks.get(index))
                        .and_then(|block| match &block.source {
                            BlockSource::Tool { name, .. } => Some(name.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| ToolName(String::new()));
                    let lines = display.project_lines(&name);
                    let fill = Fill::from_preview(&Some(display));
                    let insert_at = tool_index
                        .map(|index| index + 1)
                        .unwrap_or(self.blocks.len());
                    // Hydration rebuilds domain state from scratch; the
                    // evicted count carries no information for it.
                    let _ = self.insert_block_at(
                        insert_at,
                        Block {
                            source: BlockSource::ToolDisplay { lines },
                            lane: nu_agent_core::transcript::ir::Lane::Blank,
                            fill,
                            status: None,
                        },
                    );
                }
                continue;
            }

            if message_content.trim().is_empty() {
                continue;
            }
            if let Some(role) = message_role {
                if role == MessageRole::Assistant {
                    // Assistant prose hydrates as one whole block so
                    // multi-line markdown (tables, lists) projects intact.
                    let msg = Message {
                        role,
                        markdown: message_content.trim().to_string(),
                    };
                    let _ = self.push_block(Block {
                        source: msg.source(),
                        lane: msg.lane(),
                        fill: msg.fill(),
                        status: None,
                    });
                } else {
                    // User prompts hydrate one block per non-blank line,
                    // matching the old per-line push behavior.
                    for line in message_content.lines() {
                        if !line.trim().is_empty() {
                            let msg = Message {
                                role,
                                markdown: line.to_string(),
                            };
                            let _ = self.push_block(Block {
                                source: msg.source(),
                                lane: msg.lane(),
                                fill: msg.fill(),
                                status: None,
                            });
                        }
                    }
                }
                continue;
            }

            if message.role() == "tool" {
                let persisted = message_content.trim();
                // Resolve the (name, arguments, success) triple for the tool
                // call: explicit tool fields first, then the persisted
                // status-line format `tool[name] → args · done`.
                let resolved = message
                    .tool_arguments()
                    .map(|arguments| {
                        (
                            message
                                .tool_name()
                                .unwrap_or_else(|| extract_tool_name(persisted))
                                .to_string(),
                            arguments.to_string(),
                            message.tool_success(),
                        )
                    })
                    .or_else(|| {
                        parse_persisted_tool_status_line(persisted).map(
                            |(name, arguments, success)| {
                                (name.to_string(), arguments.to_string(), Some(success))
                            },
                        )
                    });
                let Some((name, arguments, success)) = resolved else {
                    continue;
                };
                let status = match success {
                    Some(true) => ItemStatus::Done,
                    Some(false) => ItemStatus::Failed,
                    None => ItemStatus::Unknown,
                };
                let item = Tool {
                    name: ToolName(name.clone()),
                    call: nu_agent_core::tools::handler::builtin_tool::call_line_render_for(
                        &name, &arguments,
                    ),
                    preview: None,
                    result: None,
                    status,
                };
                // Hydration rebuilds domain state from scratch; this push's
                // eviction already happened before the fresh index below is
                // recorded, so the count needs no propagation.
                let _ = self.push_block(Block {
                    source: item.source(),
                    lane: item.lane(),
                    fill: item.fill(),
                    status: Some(status),
                });
                // The fresh hydrated index is relative to the CURRENT store
                // (post-eviction); no shift is needed for it.
                tool.record_hydrated_call(&name, &arguments, self.len().saturating_sub(1), success);
                continue;
            }

            // Fallback: the old store mapped unknown roles to a system line
            // per message line; preserve that via a System notice.
            let notice = Notice {
                kind: NoticeKind::System,
                text: message_content.trim().to_string(),
            };
            let _ = self.push_block(Block {
                source: notice.source(),
                lane: notice.lane(),
                fill: notice.fill(),
                status: None,
            });
        }

        if let Some(tokens) = last_total_tokens {
            status.tokens.hydrate_latest_total_tokens(tokens);
        }
    }

    // endregion: --- Hydration
}
