//! LLM domain: LLM turn lifecycle events — phase transition on start, token
//! accounting on completion, and streaming assistant message rendering with
//! diff-regurgitation dedup.

use nu_agent_core::bus::LlmEvent;
use nu_agent_core::transcript::ir::{ContentLine, Role};
use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry, TranscriptEntryKind};

use super::transcript_store::TranscriptStore;
use super::{AppState, ScrollState, StatusState, UiPhase};

/// LLM-domain decisions extracted from the former `reduce_ui_event_impl` LLM
/// arms. The streaming cursor lives in [`TranscriptStore`]
/// ([`TranscriptStore::assistant_stream_start`]) because it is an index into
/// the transcript entries and eviction must shift it on every push.
#[derive(Debug, Clone, Default)]
pub struct LlmState;

impl LlmState {
    /// Reduce an LLM lifecycle event. Returns whether the TUI changed.
    pub fn reduce_llm_event(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        scroll: &mut ScrollState,
        phase: &mut UiPhase,
        input_locked: &mut bool,
        event: LlmEvent,
    ) -> bool {
        match event {
            LlmEvent::Started => self.handle_start(store, phase, input_locked),
            LlmEvent::Completed {
                response_chars,
                input_tokens,
                output_tokens,
                total_tokens,
                ..
            } => handle_llm_end(
                status,
                response_chars,
                input_tokens,
                output_tokens,
                total_tokens,
            ),
            LlmEvent::AssistantMessage { text } => {
                log::trace!("reducer: AssistantMessage text_len={}", text.len());
                self.assistant_message(store, scroll, &text)
            }
        }
    }

    fn handle_start(
        &mut self,
        store: &mut TranscriptStore,
        phase: &mut UiPhase,
        input_locked: &mut bool,
    ) -> bool {
        if *phase == UiPhase::Idle {
            *phase = UiPhase::Busy;
            *input_locked = true;
        }
        // Reset streaming state at the start of a new LLM response
        store.assistant_stream_start = None;
        true
    }

    fn assistant_message(
        &mut self,
        store: &mut TranscriptStore,
        scroll: &mut ScrollState,
        text: &str,
    ) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return false;
        }

        // If this is the first delta, push closing spacer for previous block (if not
        // already a Spacer) + starting spacer, and record where the message starts.
        if store.assistant_stream_start.is_none() {
            // Check if previous block was a tool block (skip spacers to find last content)
            let prev_is_tool_block = matches!(
                store.last_content_role(),
                Some(Role::Tool) | Some(Role::ToolDisplay)
            );

            if prev_is_tool_block {
                // Only ONE spacer between tool block and assistant
                store.push_spacer();
            } else {
                // Two spacers (closing + starting) for all other transitions
                // Only push a closing spacer if there is a previous block to close.
                if !store.is_empty() && !store.last_is_spacer() {
                    store.push_spacer(); // closing spacer for previous block
                }
                store.push_spacer(); // starting spacer for assistant block
            }
            store.assistant_stream_start = Some(store.len());
        }

        // Remove previous rendering of this message
        if let Some(start) = store.assistant_stream_start {
            store.truncate(start);
            store.clear_assistant_projection_cache();
        }

        // Project the full accumulated text through markdown
        let projected_for_dedup = store.project_assistant_markdown_lines(trimmed);
        if assistant_diff_regurgitation_is_redundant(store, &projected_for_dedup) {
            return false;
        }

        // Always follow tail with ListState
        scroll.scroll_transcript_to_bottom();
        store.push_transcript_item(TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Assistant(ProseMessage {
                markdown: trimmed.to_string(),
            }),
            status: None,
        });
        true
    }
}

fn handle_llm_end(
    status: &mut StatusState,
    response_chars: usize,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
) -> bool {
    status
        .tokens
        .record_token_usage(input_tokens, output_tokens, total_tokens);
    status.message.status_line = format!("Response ready ({response_chars} chars)");
    true
}

/// Single dispatch seam for the LLM domain: owns the
/// (`LlmState`, `TranscriptStore`, `StatusState`, `ScrollState`, phase,
/// input-lock) borrow split so both event paths share it. `LlmStarted` runs
/// `AppState::ensure_invariants` after the Idle→Busy transition, matching the
/// historical `handle_llm_start` (the call is a state fixpoint, so it is a
/// no-op when the phase transition did not happen).
pub(crate) fn dispatch_llm_event(state: &mut AppState, event: LlmEvent) -> bool {
    if matches!(event, LlmEvent::Started) {
        let changed = state.llm.reduce_llm_event(
            &mut state.transcript,
            &mut state.status,
            &mut state.scroll,
            &mut state.phase,
            &mut state.input_locked,
            event,
        );
        state.ensure_invariants();
        return changed;
    }
    state.llm.reduce_llm_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.scroll,
        &mut state.phase,
        &mut state.input_locked,
        event,
    )
}

// region:    --- Support

fn assistant_diff_regurgitation_is_redundant(
    store: &TranscriptStore,
    assistant_lines: &[ContentLine],
) -> bool {
    let Some(latest_tool_display_diff) = latest_tool_display_diff_lines(store) else {
        return false;
    };

    let candidate = assistant_lines
        .iter()
        .map(|cl| cl.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .map(|line| normalize_diff_line_for_comparison(line.trim()))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    if candidate.is_empty() {
        return false;
    }

    let contains_diff_signature = candidate.iter().any(|line| {
        line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@ ")
    });
    if !contains_diff_signature {
        return false;
    }

    let diff_lines = latest_tool_display_diff
        .iter()
        .map(|line| normalize_diff_line_for_comparison(line.trim()))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    candidate.iter().all(|line| {
        diff_lines.contains(line)
            || line.eq_ignore_ascii_case("dry-run diff")
            || line.eq_ignore_ascii_case("dry run diff")
            || line.ends_with(':')
    })
}

fn normalize_diff_line_for_comparison(line: &str) -> String {
    if let Some((_, rhs)) = line.split_once('│') {
        let rhs = rhs.trim_start();
        if line.starts_with('+') {
            return format!("+{rhs}");
        }
        if line.starts_with('-') {
            return format!("-{rhs}");
        }
        return format!(" {rhs}");
    }

    line.to_string()
}

fn latest_tool_display_diff_lines(store: &TranscriptStore) -> Option<Vec<String>> {
    let mut lines = Vec::new();
    for entry in store.entries().iter().rev() {
        if entry.role() == Role::ToolDisplay {
            lines.push(entry.text());
            continue;
        }

        if !lines.is_empty() {
            break;
        }
    }

    if lines.is_empty() {
        return None;
    }

    lines.reverse();
    Some(
        lines
            .into_iter()
            .filter(|line| {
                line.starts_with("--- ")
                    || line.starts_with("+++ ")
                    || line.starts_with("@@ ")
                    || line.starts_with(' ')
                    || line.starts_with('-')
                    || line.starts_with('+')
                    || line.starts_with('\\')
            })
            .collect(),
    )
}

// endregion: --- Support
