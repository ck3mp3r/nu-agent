//! Tool domain: tool-call bookkeeping, tool-display rendering, and the
//! tool-event reducer.

use std::collections::{HashMap, VecDeque};

use nu_agent_core::bus::ToolEvent;
use nu_agent_core::protocol::event::{ToolDisplay, ToolDisplaySection};
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::{ToolInvocation, TranscriptEntry, TranscriptEntryKind};
use nu_agent_core::transcript::renderer::ItemStatus;

use super::transcript_store::TranscriptStore;
use super::{AppState, ToolCallLine, TranscriptRole};

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
    /// Reduce a tool lifecycle event. Returns whether the TUI changed.
    pub fn reduce_tool_event(&mut self, store: &mut TranscriptStore, event: ToolEvent) -> bool {
        match event {
            ToolEvent::Started {
                name, arguments, ..
            } => self.tool_started(store, &name, &arguments),
            ToolEvent::Completed {
                name,
                arguments,
                success,
                display,
                ..
            } => self.tool_completed(store, &name, &arguments, success, display),
        }
    }

    fn tool_started(&mut self, store: &mut TranscriptStore, name: &str, arguments: &str) -> bool {
        self.push_block_spacers(store, name);
        self.start_tool_call(store, name, arguments);
        true
    }

    /// Push the spacers that separate a new tool block from the preceding
    /// content. The count is decided purely by [`spacer_count`]; this method
    /// only applies it.
    fn push_block_spacers(&self, store: &mut TranscriptStore, name: &str) {
        for _ in 0..spacer_count(store, name) {
            store.push_spacer();
        }
    }

    fn tool_completed(
        &mut self,
        store: &mut TranscriptStore,
        name: &str,
        arguments: &str,
        success: bool,
        display: Option<ToolDisplay>,
    ) -> bool {
        self.finish_tool_call(store, name, arguments, Some(success));

        if let Some(display) = display {
            append_direct_tool_display(store, display);
        }

        // NO push_spacer() here — tool calls within the same block have no spacers between them
        true
    }

    pub(crate) fn start_tool_call(
        &mut self,
        store: &mut TranscriptStore,
        name: &str,
        arguments: &str,
    ) {
        // The nu tool renders its raw command as a highlighted code block
        // under the status row, so the args field carries the command text
        // itself; every other tool keeps the truncated JSON summary.
        let args_summary = nu_agent_core::protocol::tool_args::summarize_tool_arguments(arguments);
        let args_display = if name == "nu" {
            nu_agent_core::protocol::tool_args::nu_command_from_args(arguments)
                .unwrap_or_else(|| format!("→ {args_summary}"))
        } else {
            format!("→ {args_summary}")
        };
        store.push_transcript_item(nu_agent_core::transcript::items::TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Tool(ToolInvocation {
                name: name.to_string(),
                source: String::new(),
                args: args_display,
            }),
            status: Some(ItemStatus::InProgress),
        });
        let entry_id = store.last_entry_id();

        super::tool_calls::ToolCallBookkeeping::new(
            &mut self.calls,
            &mut self.active_ids_by_key,
            &mut self.next_call_id,
        )
        .start_tool_call(name, arguments, entry_id);
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
        .finish_tool_call(name, arguments, success, store.entries_mut(), item_status);
    }
}

/// Whether the entry is a tool-call or tool-display row (i.e. part of a tool
/// block).
fn is_tool_entry(entry: &TranscriptEntry) -> bool {
    matches!(
        &entry.kind,
        TranscriptEntryKind::Tool(_) | TranscriptEntryKind::ToolResult(_)
    )
}

/// Whether the entry renders a full-width background block: a nu tool call, or
/// a tool-display carrying Diff* hints (edit diff).
fn renders_background_block(entry: &TranscriptEntry) -> bool {
    match &entry.kind {
        TranscriptEntryKind::Tool(inv) => inv.name == "nu",
        TranscriptEntryKind::ToolResult(result) => result.lines.iter().any(|line| {
            line.spans.iter().any(|s| {
                matches!(
                    s.hint,
                    nu_agent_core::transcript::ir::StyleHint::DiffAdd
                        | nu_agent_core::transcript::ir::StyleHint::DiffRemove
                        | nu_agent_core::transcript::ir::StyleHint::DiffHunk
                )
            })
        }),
        _ => false,
    }
}

/// Decide how many spacer rows separate a new tool block from the preceding
/// content. Pure decision: the caller applies the count.
///
/// - Continuing a tool block: 0 spacers, unless either the previous or the new
///   call renders a background block (then 1, so filled regions stay separate).
/// - New block after an assistant turn: 1 spacer (unless one was already pushed).
/// - New block after anything else: a closing spacer for the previous block
///   (if any) plus a starting spacer.
fn spacer_count(store: &TranscriptStore, name: &str) -> usize {
    let continuing_block = store.last().is_some_and(is_tool_entry);
    if continuing_block {
        let new_renders_block = name == "nu";
        let prev_renders_block = store.last().is_some_and(renders_background_block);
        return usize::from(new_renders_block || prev_renders_block);
    }

    if matches!(store.last_content_role(), Some(Role::Assistant)) {
        return usize::from(!store.last_is_spacer());
    }

    let closing = usize::from(!store.is_empty() && !store.last_is_spacer());
    closing + 1
}

/// Renders a pre-authorize tool display (e.g. an edit diff) into the
/// transcript before the user is asked to approve/deny the tool call.
///
/// No bookkeeping is needed to avoid a later duplicate: `HookChain` (in
/// nu-agent-core) already omits `display` from the matching `Completed`
/// event when a preview was shown, so `tool_completed` never sees the same
/// content twice.
pub(crate) fn note_permission_request_display(
    store: &mut TranscriptStore,
    context: &nu_agent_core::protocol::event::PermissionRequestContext,
) {
    if let Some(display) = &context.pre_authorize_display {
        append_direct_tool_display(store, display.clone());
    }
}

/// Single dispatch seam for the tool domain: owns the
/// (`ToolState`, `TranscriptStore`) borrow split so both event paths (bus
/// receivers and the protocol `UiEvent` dispatch) share it.
pub(crate) fn dispatch_tool_event(state: &mut AppState, event: ToolEvent) -> bool {
    state.tool.reduce_tool_event(&mut state.transcript, event)
}

pub(crate) fn append_direct_tool_display(
    store: &mut TranscriptStore,
    display: ToolDisplay,
) -> bool {
    let suppress_title = should_suppress_redundant_edit_title(&display);
    let suppress_single_section_stats = suppress_title && display.sections.len() == 1;

    if !suppress_title {
        store.push_transcript_line(TranscriptRole::ToolDisplay, display.title);
    }

    for section in display.sections {
        append_direct_tool_display_section(store, section, suppress_single_section_stats);
    }

    true
}

fn should_suppress_redundant_edit_title(display: &ToolDisplay) -> bool {
    display.title.starts_with("edit ")
        && display.sections.len() == 1
        && display.sections[0].language == "diff"
}

fn append_direct_tool_display_section(
    store: &mut TranscriptStore,
    section: ToolDisplaySection,
    suppress_stats_line: bool,
) {
    store.push_transcript_line(
        TranscriptRole::ToolDisplay,
        format!("{} ({})", section.label, section.language),
    );

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
            store.push_transcript_line(TranscriptRole::ToolDisplay, stat_parts.join(" "));
        }
    }

    let section_content = if section.language == "diff" {
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
    let projected = if section.language == "diff" {
        crate::markdown::project_diff_lines(&section_content)
    } else {
        crate::markdown::project_code_block_lines(&section.language, &section_content)
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
        store.push_tool_display_lines(lines);
    }
}

fn parse_hunk_start(line: &str, prefix: char) -> Option<usize> {
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch == prefix {
            let remainder = chars.as_str();
            let digits: String = remainder
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if digits.is_empty() {
                return None;
            }
            return digits.parse::<usize>().ok();
        }
    }
    None
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
            old_line = parse_hunk_start(line, '-');
            new_line = parse_hunk_start(line, '+');
            out.push_str(line);
            out.push_str(newline);
            continue;
        }

        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("\\ ") {
            out.push_str(line);
            out.push_str(newline);
            continue;
        }

        let mut chars = line.chars();
        let prefix = chars.next();
        let body = chars.as_str();

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
