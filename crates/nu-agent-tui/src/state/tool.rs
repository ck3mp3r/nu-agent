//! Tool domain: tool-call bookkeeping, tool-display rendering, and the
//! tool-event reducer.

use std::collections::{HashMap, HashSet, VecDeque};

use nu_agent_core::bus::ToolEvent;
use nu_agent_core::protocol::event::{ToolDisplay, ToolDisplaySection};
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::{ToolInvocation, TranscriptEntryKind};
use nu_agent_core::transcript::renderer::ItemStatus;

use super::transcript_store::TranscriptStore;
use super::{AppState, StatusState, ToolCallLine, ToolCallStatus, TranscriptRole};

/// Tool-domain state extracted from `AppState`: the tool-call rows tracked by
/// key, the active (in-progress) call ids per key, the next call id, and the
/// keys whose display was already rendered during a permission request.
#[derive(Debug, Clone)]
pub struct ToolState {
    pub(crate) calls: Vec<ToolCallLine>,
    pub(crate) active_ids_by_key: HashMap<String, VecDeque<u64>>,
    next_call_id: u64,
    pub(crate) pre_displayed_keys: HashSet<String>,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            active_ids_by_key: HashMap::new(),
            next_call_id: 1,
            pre_displayed_keys: HashSet::new(),
        }
    }
}

impl ToolState {
    /// Reduce a tool lifecycle event. Returns whether the TUI changed.
    pub fn reduce_tool_event(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        event: ToolEvent,
    ) -> bool {
        match event {
            ToolEvent::Started {
                name, arguments, ..
            } => self.tool_started(store, status, &name, &arguments),
            ToolEvent::Completed {
                name,
                arguments,
                success,
                display,
                ..
            } => self.tool_completed(store, status, &name, &arguments, success, display),
        }
    }

    fn tool_started(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        name: &str,
        arguments: &str,
    ) -> bool {
        // Push closing spacer for previous block (if not already a Spacer) + starting
        // spacer, but only when starting a new tool block (not continuing from a
        // previous tool call in the same block). Tool calls within a block have no
        // spacers between them.
        let is_continuing_tool_block = store.last().is_some_and(|last| {
            matches!(
                &last.kind,
                TranscriptEntryKind::Tool(_) | TranscriptEntryKind::ToolResult(_)
            )
        });
        if !is_continuing_tool_block {
            let prev_is_assistant = matches!(store.last_content_role(), Some(Role::Assistant));

            if prev_is_assistant {
                // Only ONE spacer between assistant and tool block
                // If the closing spacer was already pushed, don't add another
                if !store.last_is_spacer() {
                    store.push_spacer();
                }
            } else {
                // Two spacers (closing + starting) for all other transitions
                // Only push a closing spacer if there is a previous block to close.
                if !store.is_empty() && !store.last_is_spacer() {
                    store.push_spacer(); // closing spacer for previous block
                }
                store.push_spacer(); // starting spacer for tool block
            }
        }
        self.start_tool_call(store, name, arguments);
        status.message.status_line = format!("Tool: {name}");
        true
    }

    fn tool_completed(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        name: &str,
        arguments: &str,
        success: bool,
        display: Option<ToolDisplay>,
    ) -> bool {
        self.finish_tool_call(store, name, arguments, Some(success));

        let tool_key = format!("{name}\n{arguments}");
        if self.pre_displayed_keys.remove(&tool_key) {
            // Display was already pushed during permission request - skip
        } else if let Some(display) = display {
            append_direct_tool_display(store, display);
        }

        // NO push_spacer() here — tool calls within the same block have no spacers between them
        status.message.status_line = "Thinking...".to_string();
        true
    }

    pub(crate) fn start_tool_call(
        &mut self,
        store: &mut TranscriptStore,
        name: &str,
        arguments: &str,
    ) {
        let args_summary = nu_agent_core::protocol::tool_args::summarize_tool_arguments(arguments);
        store.push_transcript_item(nu_agent_core::transcript::items::TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Tool(ToolInvocation {
                name: name.to_string(),
                source: String::new(),
                args: format!("→ {args_summary}"),
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

    pub(crate) fn latest_in_progress_tool_key_for_tool(&self, tool_name: &str) -> Option<String> {
        let base_tool_name = tool_name.split('(').next().unwrap_or(tool_name);

        self.calls
            .iter()
            .rev()
            .find(|item| {
                item.status == ToolCallStatus::InProgress
                    && item
                        .key
                        .split_once('\n')
                        .map(|(name, _)| name == base_tool_name)
                        .unwrap_or(false)
            })
            .map(|item| item.key.clone())
    }
}

/// Renders a pre-authorize tool display into the transcript and records the
/// matching in-progress tool key so the completion event skips the duplicate.
pub(crate) fn note_permission_request_display(
    tool: &mut ToolState,
    store: &mut TranscriptStore,
    context: &nu_agent_core::protocol::event::PermissionRequestContext,
) {
    if let Some(display) = &context.pre_authorize_display {
        append_direct_tool_display(store, display.clone());

        if let Some(tool_key) = tool.latest_in_progress_tool_key_for_tool(&context.tool) {
            tool.pre_displayed_keys.insert(tool_key);
        }
    }
}

/// Single dispatch seam for the tool domain: owns the
/// (`ToolState`, `TranscriptStore`, `StatusState`) borrow split so both event
/// paths (bus receivers and the protocol `UiEvent` dispatch) share it.
pub(crate) fn dispatch_tool_event(state: &mut AppState, event: ToolEvent) -> bool {
    state
        .tool
        .reduce_tool_event(&mut state.transcript, &mut state.status, event)
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

    let markdown = format!("```{}\n{}\n```", section.language, section_content);
    for rendered_line in store.project_assistant_markdown_lines(&markdown) {
        let text: String = rendered_line
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        if text.trim().is_empty() {
            continue;
        }
        store.push_transcript_line(TranscriptRole::ToolDisplay, text);
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
