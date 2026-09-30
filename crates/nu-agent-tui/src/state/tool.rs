//! tool-event reducer.

use std::collections::{HashMap, VecDeque};

use nu_agent_core::bus::ToolEvent;
use nu_agent_core::protocol::event::{ToolDisplay, ToolDisplaySection};
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::Block;
use nu_agent_core::transcript::ir::BlockSource;
use nu_agent_core::transcript::ir::ContentKind;
use nu_agent_core::transcript::ir::Fill;
use nu_agent_core::transcript::ir::ToolName;
use nu_agent_core::transcript::items::Tool;
use nu_agent_core::transcript::renderer::ItemStatus;
use nu_agent_core::transcript::renderer::Renderable;

use super::transcript_store::TranscriptStore;
use super::{AppState, ToolCallLine, ToolCallStatus};

/// Tool-domain state extracted from `AppState`: the tool-call rows tracked by
/// key, the active (in-progress) call ids per key, and the next call id.
///
/// Completion-display de-duplication against a pre-authorize preview is NOT
/// handled here: `HookChain::on_tool_result`/`suppress_previewed_display`
/// (nu-agent-core) already omit `display` from the `Completed` event when a
/// preview was shown, so this layer never sees a duplicate to suppress.
#[derive(Debug, Clone)]
pub struct ToolState {
    pub(crate) calls: Vec<ToolCallLine>,
    pub(crate) active_ids_by_key: HashMap<String, VecDeque<u64>>,
    next_call_id: u64,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            active_ids_by_key: HashMap::new(),
            next_call_id: 1,
        }
    }
}

impl ToolState {
    /// Reduce a tool lifecycle event. Returns whether the TUI changed. The
    /// evicted count from any block push inside is returned via `evicted` so
    /// the top-level caller can shift domain bookkeeping.
    pub fn reduce_tool_event(
        &mut self,
        store: &mut TranscriptStore,
        event: ToolEvent,
        evicted: &mut usize,
    ) -> bool {
        match event {
            ToolEvent::Started {
                name,
                arguments,
                call_line,
                ..
            } => self.tool_started(store, &name, &arguments, call_line, evicted),
            ToolEvent::Completed {
                name,
                arguments,
                success,
                display,
                ..
            } => self.tool_completed(store, &name, &arguments, success, display, evicted),
        }
    }

    fn tool_started(
        &mut self,
        store: &mut TranscriptStore,
        name: &str,
        arguments: &str,
        call_line: CallLine,
        evicted: &mut usize,
    ) -> bool {
        self.start_tool_call(store, name, arguments, call_line, evicted);
        true
    }

    fn tool_completed(
        &mut self,
        store: &mut TranscriptStore,
        name: &str,
        arguments: &str,
        success: bool,
        display: Option<ToolDisplay>,
        evicted: &mut usize,
    ) -> bool {
        self.finish_tool_call(store, name, arguments, Some(success));

        if let Some(display) = display {
            *evicted += append_direct_tool_display(store, name, display);
        }

        true
    }

    pub(crate) fn start_tool_call(
        &mut self,
        store: &mut TranscriptStore,
        name: &str,
        arguments: &str,
        call_line: CallLine,
        evicted: &mut usize,
    ) {
        let status = ItemStatus::InProgress;
        let tool = Tool {
            name: ToolName(name.to_string()),
            call: call_line,
            preview: None,
            result: None,
            status,
        };
        *evicted += store.push_block(Block {
            source: tool.source(),
            lane: tool.lane(),
            fill: tool.fill(),
            status: Some(status),
        });
        // Pre-compensate for this push's evictions: the dispatch seam shifts
        // ALL bookkeeping by the accumulated evicted count after the event,
        // so the fresh entry must be stored in pre-shift coordinates to land
        // on its true (post-eviction) block.
        let block_index = store.len().saturating_sub(1) + *evicted;

        super::tool_calls::ToolCallBookkeeping::new(
            &mut self.calls,
            &mut self.active_ids_by_key,
            &mut self.next_call_id,
        )
        .start_tool_call(name, arguments, block_index);
    }

    /// Attach a pre-execution preview (edit diff, nu code) to the pending tool
    /// call as its own `ToolDisplay` block, pushed immediately after the Tool
    /// block. The Tool block keeps `preview: None` and `Fill::None` for its
    /// whole life, so the call line never gets the code background or a margin
    /// row; only the preview block carries `Fill::Code`.
    ///
    /// The preview block's lines come from [`Display::project_lines`] — the
    /// same projection the Tool block's preview path uses — so the redundant
    /// title/label/stats suppression cannot drift between the two.
    ///
    /// The push can evict blocks at the transcript cap; the evicted count is
    /// returned via `evicted` so the caller shifts domain bookkeeping.
    pub(crate) fn set_tool_preview(
        &mut self,
        store: &mut TranscriptStore,
        name: &str,
        arguments: &str,
        preview: nu_agent_core::transcript::ir::Display,
        evicted: &mut usize,
    ) {
        let block_index = self
            .calls
            .iter()
            .rev()
            .find(|tool| tool.key == format!("{name}\n{arguments}"))
            .and_then(|tool| tool.block_index);
        let Some(block_index) = block_index else {
            return;
        };

        let lines = preview.project_lines(&ToolName(name.to_string()));
        let fill = Fill::from_preview(&Some(preview));
        *evicted += store.insert_block_at(
            block_index.saturating_add(1),
            Block {
                source: BlockSource::ToolDisplay { lines },
                lane: nu_agent_core::transcript::ir::Lane::Blank,
                fill,
                status: None,
            },
        );

        // The preview block added rendered rows. Mark the height index stale so
        // the render loop re-measures and actually renders them (task 5c0edbe9).
        store.invalidate_height_index();
    }

    /// Record a hydrated (already-finished) tool call in the bookkeeping so
    /// later lookups by name+arguments find its block index. The entry is
    /// terminal from birth — it never enters the active deque, so a later
    /// live call with the same key cannot steal its finish.
    pub(crate) fn record_hydrated_call(
        &mut self,
        name: &str,
        arguments: &str,
        block_index: usize,
        success: Option<bool>,
    ) {
        super::tool_calls::ToolCallBookkeeping::new(
            &mut self.calls,
            &mut self.active_ids_by_key,
            &mut self.next_call_id,
        )
        .record_terminal_call(name, arguments, block_index, success);
    }

    pub(crate) fn finish_tool_call(
        &mut self,
        store: &mut TranscriptStore,
        name: &str,
        arguments: &str,
        success: Option<bool>,
    ) {
        let item_status = match success {
            Some(true) => ItemStatus::Done,
            Some(false) => ItemStatus::Failed,
            None => ItemStatus::Unknown,
        };
        super::tool_calls::ToolCallBookkeeping::new(
            &mut self.calls,
            &mut self.active_ids_by_key,
            &mut self.next_call_id,
        )
        .finish_tool_call(name, arguments, success, store.blocks_mut(), item_status);
    }
}

/// Attach a pre-authorize tool display (e.g. an edit diff) as its own
/// `ToolDisplay` block after the pending tool call's block, before the user is
/// asked to approve/deny. Returns the number of blocks evicted by the push so
/// the caller can shift domain bookkeeping.
///
/// No bookkeeping is needed to avoid a later duplicate: `HookChain` (in
/// nu-agent-core) already omits `display` from the matching `Completed`
/// event when a preview was shown, so `tool_completed` never sees the same
/// content twice.
pub(crate) fn note_permission_request_display(
    state: &mut AppState,
    context: &nu_agent_core::protocol::event::PermissionRequestContext,
) -> usize {
    let Some(display) = &context.pre_authorize_display else {
        return 0;
    };
    note_tool_preview(state, &context.tool_key, display)
}

/// Attach a pre-authorize tool display to the pending tool call identified by
/// `tool_key` (`{tool_name}\n{raw_arguments}`). Returns the number of blocks
/// evicted by the push so the caller can shift domain bookkeeping. A key that
/// matches no pending call pushes nothing.
pub(crate) fn note_tool_preview(
    state: &mut AppState,
    tool_key: &str,
    display: &ToolDisplay,
) -> usize {
    // The key is the exact call key (`{name}\n{arguments}`), so the pending
    // call is matched byte-for-byte. A decorated display name never equals a
    // call key.
    let Some(pending) = state
        .tool
        .calls
        .iter()
        .rev()
        .find(|call| call.status == ToolCallStatus::InProgress && call.key == tool_key)
    else {
        return 0;
    };
    let (name, arguments) = pending
        .key
        .split_once('\n')
        .map(|(n, a)| (n.to_string(), a.to_string()))
        .unwrap_or((tool_key.to_string(), String::new()));
    let mut evicted = 0usize;
    state.tool.set_tool_preview(
        &mut state.transcript,
        &name,
        &arguments,
        preview_to_display(display),
        &mut evicted,
    );
    evicted
}

/// Convert a protocol event display into the IR display type. The shapes are
/// identical; the IR type keeps the renderer independent of the event layer.
pub(crate) fn preview_to_display(display: &ToolDisplay) -> nu_agent_core::transcript::ir::Display {
    nu_agent_core::transcript::ir::Display {
        title: display.title.clone(),
        sections: display
            .sections
            .iter()
            .map(|section| nu_agent_core::transcript::ir::DisplaySection {
                label: section.label.clone(),
                kind: section.kind.clone(),
                content: section.content.clone(),
                stats: section.stats.clone(),
            })
            .collect(),
    }
}

/// Single dispatch seam for the tool domain: owns the
/// (`ToolState`, `TranscriptStore`) borrow split so both event paths (bus
/// receivers and the protocol `UiEvent` dispatch) share it. Any eviction
/// caused by the event's pushes shifts the domain block_index bookkeeping.
pub(crate) fn dispatch_tool_event(state: &mut AppState, event: ToolEvent) -> bool {
    let mut evicted = 0usize;
    let changed = state
        .tool
        .reduce_tool_event(&mut state.transcript, event, &mut evicted);
    state.shift_bookkeeping_after_eviction(evicted);
    changed
}

pub(crate) fn append_direct_tool_display(
    store: &mut TranscriptStore,
    tool_name: &str,
    display: ToolDisplay,
) -> usize {
    let suppress_title = should_suppress_redundant_edit_title(tool_name, &display);
    let suppress_single_section_stats = suppress_title && display.sections.len() == 1;
    // The call line already shows `→ <path> (diff)`, so the section label row
    // is redundant for the same single-diff-section edit display whose title
    // is suppressed.
    let skip_section_label = suppress_title;

    let mut evicted = 0usize;
    if !suppress_title {
        evicted += push_tool_display_text_block(store, &display.title);
    }

    for section in display.sections {
        evicted += append_direct_tool_display_section(
            store,
            section,
            suppress_single_section_stats,
            skip_section_label,
        );
    }

    evicted
}

/// The redundant-title suppression is decided from TYPED tool identity:
/// `BuiltinKind::Edit` (parsed from the event's tool name) plus the display
/// shape — never from the title text. A title like `edit notes/todo.md`
/// must not influence the decision; only the tool being `edit` does.
///
/// Tool identity comes from [`ToolName::is_edit`] (the name type owns its own
/// classification) and display shape from `Display::is_single_diff_section`,
/// so the completion path and the preview projection (which applies the same
/// rule) cannot drift apart.
fn should_suppress_redundant_edit_title(tool_name: &str, display: &ToolDisplay) -> bool {
    ToolName(tool_name.to_string()).is_edit()
        && preview_to_display(display).is_single_diff_section()
}

fn is_diff_kind(kind: &ContentKind) -> bool {
    matches!(kind, ContentKind::Diff { .. })
}

fn append_direct_tool_display_section(
    store: &mut TranscriptStore,
    section: ToolDisplaySection,
    suppress_stats_line: bool,
    skip_section_label: bool,
) -> usize {
    let mut evicted = 0usize;
    if !skip_section_label {
        evicted += push_tool_display_text_block(
            store,
            &format!("{} ({})", section.label, section.kind.language()),
        );
    }

    if !suppress_stats_line && let Some(stats) = section.stats {
        let mut stat_parts = Vec::new();
        if let Some(files_changed) = stats.files_changed {
            stat_parts.push(format!("files={files_changed}"));
        }
        if let Some(insertions) = stats.insertions {
            stat_parts.push(format!("+{insertions}"));
        }
        if let Some(deletions) = stats.deletions {
            stat_parts.push(format!("-{deletions}"));
        }
        if let Some(true) = stats.diff_truncated {
            stat_parts.push("truncated=true".to_string());
        }
        if !stat_parts.is_empty() {
            push_tool_display_text_block(store, &stat_parts.join(" "));
        }
    }

    let section_content = if is_diff_kind(&section.kind) {
        add_diff_line_number_readability(&section.content)
    } else {
        section.content
    };

    // Project the section content directly so ContentLines carry StyleHints
    // instead of being flattened to plain text. Diffs take the dedicated
    // annotate_diff_hint path (DiffAdd/DiffRemove/DiffHunk) — routing them
    // through syntect would flatten every line to MdCode* hints and lose the
    // diff coloring. Non-diff languages keep code-block highlighting. Both
    // paths avoid the markdown round-trip that could leak literal ``` fence
    // markers when projection falls back.
    let projected = if is_diff_kind(&section.kind) {
        crate::markdown::project_diff_lines(&section_content)
    } else {
        crate::markdown::project_code_block_lines(section.kind.language(), &section_content)
    };
    let mut lines = Vec::with_capacity(projected.len());
    for rendered_line in projected {
        let text: String = rendered_line
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        if text.trim().is_empty() {
            continue;
        }
        lines.push(rendered_line);
    }
    if !lines.is_empty() {
        evicted += store.push_block(Block {
            source: BlockSource::ToolDisplay { lines },
            lane: nu_agent_core::transcript::ir::Lane::Blank,
            fill: Fill::None,
            status: None,
        });
    }
    evicted
}

/// Push one ToolDisplay block whose lines are projected from a plain text
/// row (title, section label, or stats line). The markdown projection adds
/// diff hints so persisted/plain text rows keep their coloring. Returns the
/// evicted count.
fn push_tool_display_text_block(store: &mut TranscriptStore, text: &str) -> usize {
    let mut lines = crate::markdown::project_diff_lines(text);
    if lines.is_empty() {
        lines = vec![nu_agent_core::transcript::ir::ContentLine::single(
            text.to_string(),
            nu_agent_core::transcript::items::annotate_diff_hint(text),
        )];
    }
    store.push_block(Block {
        source: BlockSource::ToolDisplay { lines },
        lane: nu_agent_core::transcript::ir::Lane::Blank,
        fill: Fill::None,
        status: None,
    })
}

fn add_diff_line_number_readability(diff: &str) -> String {
    let mut old_line: Option<usize> = None;
    let mut new_line: Option<usize> = None;
    let mut out = String::new();

    for segment in diff.split_inclusive('\n') {
        let (line, newline) = if let Some(stripped) = segment.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (segment, "")
        };

        if line.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_header_start(line) {
                old_line = Some(old_start);
                new_line = Some(new_start);
            }
            out.push_str(line);
            out.push_str(newline);
            continue;
        }

        // Unified-diff file headers and the no-newline marker are structure,
        // not hunk content — they carry no line numbers.
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("\\ ") {
            out.push_str(line);
            out.push_str(newline);
            continue;
        }

        let mut chars = line.chars();
        let prefix = chars.next();
        let body = chars.as_str();

        // Context lines show both numbers; removed lines advance the old
        // counter; added lines advance the new counter. Numbers are
        // right-aligned to 4 columns and the pipe sits against the body.
        match (prefix, old_line, new_line) {
            (Some(' '), Some(old), Some(new)) => {
                out.push_str(&format!(" {old:>4} {new:>4} │{body}{newline}"));
                old_line = Some(old.saturating_add(1));
                new_line = Some(new.saturating_add(1));
            }
            (Some('-'), Some(old), _) => {
                out.push_str(&format!("-{old:>4}      │{body}{newline}"));
                old_line = Some(old.saturating_add(1));
            }
            (Some('+'), _, Some(new)) => {
                out.push_str(&format!("+     {new:>4} │{body}{newline}"));
                new_line = Some(new.saturating_add(1));
            }
            _ => {
                out.push_str(line);
                out.push_str(newline);
            }
        }
    }

    out
}

fn parse_hunk_header_start(line: &str) -> Option<(usize, usize)> {
    let mut parts = line.split_whitespace();
    let old = parts.nth(1)?;
    let new = parts.next()?;
    let old_start = old
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse::<usize>()
        .ok();
    let new_start = new
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse::<usize>()
        .ok();
    Some((old_start?, new_start?))
}

// region:    --- Tests

#[cfg(test)]
#[path = "tool_diff_test.rs"]
mod tool_diff_tests;

// endregion: --- Tests
