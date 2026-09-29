//! LLM domain: LLM turn lifecycle events — phase transition on start, token
//! accounting on completion, and streaming assistant message rendering with
//! diff-regurgitation dedup.

use nu_agent_core::bus::LlmEvent;
use nu_agent_core::transcript::ir::{Block, ContentLine, MessageRole};
use nu_agent_core::transcript::items::Message;
use nu_agent_core::transcript::renderer::Renderable;

use super::transcript_store::TranscriptStore;
use super::{AppState, ScrollState, StatusState, UiPhase};

/// LLM-domain decisions extracted from the former `reduce_ui_event_impl` LLM
/// arms. The streaming cursor lives in [`TranscriptStore`]
/// ([`TranscriptStore::assistant_stream_start`]) because it is an index into
/// the transcript entries and eviction must shift it on every push.
#[derive(Debug, Clone, Default)]
pub struct LlmState;

impl LlmState {
    /// Reduce an LLM lifecycle event. Returns `(changed, evicted)` — whether
    /// the TUI changed and the evicted count from any block push inside, so
    /// the top-level caller can shift domain bookkeeping.
    pub fn reduce_llm_event(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        scroll: &mut ScrollState,
        phase: &mut UiPhase,
        input_locked: &mut bool,
        event: LlmEvent,
    ) -> (bool, usize) {
        let mut evicted = 0usize;
        let changed = match event {
            LlmEvent::Started => self.handle_start(store, phase, input_locked),
            LlmEvent::Completed {
                input_tokens,
                output_tokens,
                total_tokens,
                ..
            } => handle_llm_end(status, input_tokens, output_tokens, total_tokens),
            LlmEvent::AssistantMessage { text } => {
                log::trace!("reducer: AssistantMessage text_len={}", text.len());
                self.assistant_message(store, scroll, &text, &mut evicted)
            }
            LlmEvent::Stopped { reason } => self.stopped(store, scroll, &reason, &mut evicted),
        };
        (changed, evicted)
    }

    /// Render a hook-stop reason as a closing notice. The reason is APPENDED
    /// as a new transcript entry — never routed through the
    /// `assistant_message` dedup path, which would truncate the streamed
    /// block (Fix 2 of the repetition stop UX). The streaming cursor is
    /// cleared so the streamed block stays intact and any later assistant
    /// message starts a fresh block.
    fn stopped(
        &mut self,
        store: &mut TranscriptStore,
        scroll: &mut ScrollState,
        reason: &str,
        evicted: &mut usize,
    ) -> bool {
        let trimmed = reason.trim();
        if trimmed.is_empty() {
            // Silent discard: an empty reason signals that the in-progress
            // streaming block is provisional output from a retried turn
            // (rig's `ModelTurnRetried` contract). Remove the block instead
            // of leaving it orphaned next to the retry's output.
            if let Some(start) = store.assistant_stream_start {
                store.truncate(start);
            }
            store.assistant_stream_start = None;
            return false;
        }
        // Close the in-progress streamed block: push_block's unified spacer
        // rule separates it from the notice, and the cursor resets so nothing
        // later truncates it.
        store.assistant_stream_start = None;
        scroll.scroll_transcript_to_bottom();
        let msg = Message {
            role: MessageRole::Assistant,
            markdown: trimmed.to_string(),
        };
        *evicted += store.push_block(Block {
            source: msg.source(),
            lane: msg.lane(),
            fill: msg.fill(),
            status: None,
        });
        true
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
        evicted: &mut usize,
    ) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return false;
        }

        // If this is the first delta, record where the message starts.
        // push_block's unified spacer rule separates the streamed block from
        // whatever precedes it (tool block or previous turn).
        if store.assistant_stream_start.is_none() {
            store.assistant_stream_start = Some(store.len());
        }

        // Remove previous rendering of this message
        if let Some(start) = store.assistant_stream_start {
            store.truncate(start);
        }

        // Project the full accumulated text through markdown for the dedup
        // check against the latest tool-display diff.
        let projected_for_dedup = crate::markdown::render_markdown_lines(trimmed, None);
        if assistant_diff_regurgitation_is_redundant(store, &projected_for_dedup) {
            return false;
        }

        // Always follow tail with ListState
        scroll.scroll_transcript_to_bottom();
        let msg = Message {
            role: MessageRole::Assistant,
            markdown: trimmed.to_string(),
        };
        *evicted += store.push_block(Block {
            source: msg.source(),
            lane: msg.lane(),
            fill: msg.fill(),
            status: None,
        });
        true
    }
}

fn handle_llm_end(
    status: &mut StatusState,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
) -> bool {
    status
        .tokens
        .record_token_usage(input_tokens, output_tokens, total_tokens);
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
        let (changed, evicted) = state.llm.reduce_llm_event(
            &mut state.transcript,
            &mut state.status,
            &mut state.scroll,
            &mut state.phase,
            &mut state.input_locked,
            event,
        );
        state.ensure_invariants();
        state.shift_bookkeeping_after_eviction(evicted);
        return changed;
    }
    let (changed, evicted) = state.llm.reduce_llm_event(
        &mut state.transcript,
        &mut state.status,
        &mut state.scroll,
        &mut state.phase,
        &mut state.input_locked,
        event,
    );
    state.shift_bookkeeping_after_eviction(evicted);
    changed
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

// endregion: --- Support

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
    for block in store.blocks().iter().rev() {
        let text = block.source.plain_text();
        if text.trim().is_empty() {
            continue;
        }
        lines.push(text);

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
