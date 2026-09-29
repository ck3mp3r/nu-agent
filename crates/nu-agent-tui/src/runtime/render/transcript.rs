use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::runtime::render::expand_to_visual_rows;
use crate::runtime::transcript_entries_for_render;
use crate::tui_renderer::layout;
use crate::{
    runtime::render::frame::current_time_millis,
    state::{InputMode, PaneFocus},
};
use nu_agent_core::transcript::ir::{BlockSource, Fill, MessageRole};

use crate::runtime::RuntimeCoordinator;

impl RuntimeCoordinator {
    pub(crate) fn render_transcript_pane(
        &mut self,
        frame: &mut Frame,
        transcript_content_area: Rect,
        transcript_list_area: Rect,
        transcript_following_tail: bool,
        transcript_scroll_offset: usize,
        rendered_scroll_offset: &mut Option<usize>,
    ) {
        let now_millis = current_time_millis();

        frame.render_widget(Clear, transcript_content_area);
        frame.render_widget(
            Block::default().style(Style::default().bg(self.theme.base)),
            transcript_content_area,
        );
        if transcript_content_area.height > 0 {
            let width = transcript_list_area.width as usize;
            let viewport_height = transcript_list_area.height as usize;

            // Rebuild the height index when stale (entries added, evicted, or resize)
            if !self.state.transcript.height_index_valid_for(width) {
                self.state.transcript.rebuild_height_index(width);
            }

            let blocks = transcript_entries_for_render(&self.state).to_vec();

            // Total visual rows from the measure()-based height index.
            let total_visual_rows = self.state.transcript.total_visual_rows();

            self.state.scroll.viewport_height = viewport_height;
            let max_scroll = self.state.scroll.sync_after_render(total_visual_rows);
            let effective_offset: usize = if transcript_following_tail {
                max_scroll
            } else {
                transcript_scroll_offset.min(max_scroll)
            };
            *rendered_scroll_offset = Some(effective_offset);
            // When following tail, sync_after_render keeps the cursor at the
            // last visual row; when not tailing, cursor_visual_row is
            // user-controlled — untouched.

            // Binary search for visible entries via the height index.
            let (first_visible, last_visible) = self
                .state
                .transcript
                .visible_window(effective_offset, viewport_height);

            // Only render visible blocks, inserting the separator rows the
            // state machine decides between adjacent blocks. The machine is
            // seeded from the block just above the first visible one so a
            // scrolled view keeps the same separation as a full view.
            let mut all_lines: Vec<Line<'static>> = Vec::new();
            let mut block_indices: Vec<usize> = Vec::new();
            // Per-line full-width background override for separator rows.
            // `None` means "resolve from the block via full_width_background";
            // `Some(color)` pins the row (used so a user prompt's background
            // bleeds across its neighbouring separator rows, preserving the
            // pre-refactor Fill bleed — Fill behavior is out of scope here).
            let mut line_bgs: Vec<Option<ratatui::style::Color>> = Vec::new();

            let mut sm = match first_visible.checked_sub(1).and_then(|i| blocks.get(i)) {
                Some(prev) => crate::state::spacer::SpacerStateMachine::seeded(
                    prev.source.family(),
                    prev.has_filled_content(),
                ),
                None => crate::state::spacer::SpacerStateMachine::default(),
            };

            let visible_end = last_visible.min(blocks.len());
            for (rel_idx, block) in blocks[first_visible..visible_end].iter().enumerate() {
                let idx = first_visible + rel_idx;
                let separators =
                    sm.separators_for(block.source.family(), block.has_filled_content());
                // Separator rows keep the pre-refactor bleed: the first row of
                // a run bleeds the user background when the preceding block is
                // a user turn; the last row bleeds when the incoming block is a
                // user turn. Other rows stay on the base background.
                let prev_is_user = idx
                    .checked_sub(1)
                    .and_then(|i| blocks.get(i))
                    .is_some_and(is_user_block);
                let incoming_is_user = is_user_block(block);
                for s in 0..separators {
                    let bleed =
                        (s == 0 && prev_is_user) || (s == separators - 1 && incoming_is_user);
                    let bg = if bleed {
                        self.theme.row_user_bg
                    } else {
                        self.theme.base
                    };
                    all_lines.push(Line::from(""));
                    block_indices.push(idx);
                    line_bgs.push(Some(bg));
                }
                let ctx = nu_agent_core::transcript::renderer::FrameContext {
                    width,
                    now_millis,
                    cursor: false,
                    selected: false,
                };
                let entry_lines = layout(block, &ctx);
                for _ in 0..entry_lines.len() {
                    block_indices.push(idx);
                    line_bgs.push(None);
                }
                all_lines.extend(entry_lines);
            }

            // Trailing separator rows after the final block, but only when the
            // viewport reaches the end of the transcript. A user turn relies on
            // separators for its visual margin (`Fill::Full` has no margin
            // rows), so a user block that is the last block keeps a closing
            // blank row below it (task 670e0292). When the viewport is scrolled
            // short of the end, the following block's leading separators
            // provide the gap instead, so no trailing row is emitted here. The
            // trailing row maps to the final block and bleeds the user
            // background, matching the separator-row paint above.
            if visible_end == blocks.len() && visible_end > 0 {
                let trailing = sm.trailing_separators();
                if trailing > 0 {
                    let last_idx = visible_end - 1;
                    let last_is_user = blocks.get(last_idx).is_some_and(is_user_block);
                    let bg = if last_is_user {
                        self.theme.row_user_bg
                    } else {
                        self.theme.base
                    };
                    for _ in 0..trailing {
                        all_lines.push(Line::from(""));
                        block_indices.push(last_idx);
                        line_bgs.push(Some(bg));
                    }
                }
            }

            // Bottom-align when content is shorter than viewport.
            // On fresh sessions with just a logo, this pushes the logo to the bottom
            // so the first prompt appears just above the input box.
            if total_visual_rows < viewport_height {
                let padding = viewport_height - total_visual_rows;
                let top_padding = padding / 2;
                let bottom_padding = padding - top_padding;
                let mut padded_lines = Vec::with_capacity(viewport_height);
                let mut padded_indices = Vec::with_capacity(viewport_height);
                let mut padded_bgs = Vec::with_capacity(viewport_height);
                for _ in 0..top_padding {
                    padded_lines.push(Line::from(""));
                    padded_indices.push(0);
                    padded_bgs.push(None);
                }
                padded_lines.append(&mut all_lines);
                padded_indices.append(&mut block_indices);
                padded_bgs.append(&mut line_bgs);
                for _ in 0..bottom_padding {
                    padded_lines.push(Line::from(""));
                    padded_indices.push(0);
                    padded_bgs.push(None);
                }
                all_lines = padded_lines;
                block_indices = padded_indices;
                line_bgs = padded_bgs;
            }

            // Expand block_indices to visual rows for cursor/selection mapping
            let expanded_block_indices = expand_to_visual_rows(block_indices, &all_lines, width);
            self.state.scroll.entry_indices = expanded_block_indices;
            // Expand the per-line background overrides to visual rows the same
            // way, so a wrapped line's every visual row keeps its pinned
            // background.
            let expanded_line_bgs =
                crate::runtime::render::expand_bgs_to_visual_rows(&line_bgs, &all_lines, width);
            let partial_offset =
                effective_offset.saturating_sub(self.state.transcript.start_row_of(first_visible));
            let paragraph = Paragraph::new(ratatui::text::Text::from(all_lines))
                .wrap(Wrap::default())
                .scroll((partial_offset.min(u16::MAX as usize) as u16, 0));
            frame.render_widget(paragraph, transcript_list_area);

            // Fill user prompt rows (and adjacent spacers) and code-block rows
            // with full-width background. Colors resolve from the theme: the
            // code surface from `surface0`, the user rail from `row_user_bg`.
            let full_width_bg = self.theme.row_user_bg;
            let code_surface_bg = self.theme.surface0;
            for row in 0..viewport_height {
                let Some(&block_idx) = self.state.scroll.entry_indices.get(partial_offset + row)
                else {
                    continue;
                };
                // A pinned separator-row background takes precedence; otherwise
                // resolve from the block's fill/adjacency.
                let bg = expanded_line_bgs
                    .get(partial_offset + row)
                    .copied()
                    .flatten()
                    .or_else(|| {
                        full_width_background(&blocks, block_idx, full_width_bg, code_surface_bg)
                    });
                let Some(bg) = bg else {
                    continue;
                };
                let row_screen_y = transcript_list_area.y + row as u16;
                for x in transcript_list_area.x..transcript_list_area.x + transcript_list_area.width
                {
                    if let Some(cell) = frame
                        .buffer_mut()
                        .cell_mut(ratatui::layout::Position { x, y: row_screen_y })
                    {
                        let current_style = cell.style();
                        cell.set_style(current_style.bg(bg));
                    }
                }
            }

            // Store rendered text per visible viewport row for yank support
            // Only scan the buffer in Visual mode to avoid per-frame O(width*height) cost.
            if super::should_scan_for_yank(self.state.input.mode) {
                let mut rendered_text: Vec<String> = Vec::with_capacity(viewport_height);
                for row in 0..viewport_height {
                    let row_screen_y = transcript_list_area.y + row as u16;
                    let mut row_text = String::new();
                    for x in
                        transcript_list_area.x..transcript_list_area.x + transcript_list_area.width
                    {
                        if let Some(cell) = frame.buffer_mut().cell((x, row_screen_y)) {
                            let ch = cell.symbol().chars().next().unwrap_or(' ');
                            row_text.push(ch);
                        }
                    }
                    rendered_text.push(row_text.trim_end().to_string());
                }
                self.state.scroll.rendered_line_text = rendered_text;
                self.state.scroll.rendered_line_start_row = effective_offset;
            }

            // Post-render buffer manipulation: apply selection highlight to visual rows
            if self.state.input.mode == InputMode::Visual
                && let Some(sel) = &self.state.scroll.selection
            {
                let (sel_start, sel_end) = sel.normalized_range();
                Self::apply_selection_highlight(
                    frame.buffer_mut(),
                    transcript_list_area,
                    sel_start,
                    sel_end,
                    effective_offset,
                    viewport_height,
                    self.theme.selection_bg,
                );
            }

            // Overlay > cursor indicator at the correct screen position
            if self.state.scroll.pane_focus == PaneFocus::Transcript
                && self.state.scroll.cursor_visual_row >= effective_offset
                && self.state.scroll.cursor_visual_row < effective_offset + viewport_height
                && (self.state.input.mode == InputMode::Normal
                    || self.state.input.mode == InputMode::Visual)
            {
                let cursor_y = (self.state.scroll.cursor_visual_row - effective_offset) as u16;
                let cursor_screen_y = transcript_list_area.y + cursor_y;
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled("> ", self.theme.focus))),
                    Rect::new(transcript_list_area.x, cursor_screen_y, 2, 1),
                );
            }

            if total_visual_rows > viewport_height {
                let mut scrollbar_state =
                    ScrollbarState::new(max_scroll).position(effective_offset);
                frame.render_stateful_widget(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(None)
                        .end_symbol(None)
                        .thumb_style(self.theme.focus)
                        .track_style(self.theme.subtle_meta),
                    transcript_content_area,
                    &mut scrollbar_state,
                );
            }
        }
    }
}

/// Full-width background color for the block at `block_idx`, or `None` when
/// the block needs no full-width paint. Type-driven: `Fill::Code` → code
/// surface; a User turn, or a Spacer adjacent to a User turn, → user
/// background. Everything else → `None`. The two theme colors are passed in
/// by the caller so the decision stays independent of theme wiring.
pub(super) fn full_width_background(
    blocks: &[nu_agent_core::transcript::ir::Block],
    block_idx: usize,
    full_width_bg: ratatui::style::Color,
    code_surface_bg: ratatui::style::Color,
) -> Option<ratatui::style::Color> {
    let block = blocks.get(block_idx)?;
    match block.fill {
        Fill::Code => return Some(code_surface_bg),
        Fill::Full => return Some(full_width_bg),
        Fill::None => {}
    }
    // Fill::None Spacer: only paints when adjacent to a User turn.
    if !matches!(block.source, BlockSource::Spacer) {
        return None;
    }
    let prev_is_user = block_idx
        .checked_sub(1)
        .and_then(|i| blocks.get(i))
        .is_some_and(is_user_block);
    let next_is_user = blocks.get(block_idx + 1).is_some_and(is_user_block);
    (prev_is_user || next_is_user).then_some(full_width_bg)
}

fn is_user_block(block: &nu_agent_core::transcript::ir::Block) -> bool {
    matches!(
        block.source,
        BlockSource::Markdown {
            role: MessageRole::User,
            ..
        }
    )
}
