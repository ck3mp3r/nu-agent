//! LLM domain: LLM turn lifecycle events — phase transition on start, token
//! accounting on completion, and streaming assistant message rendering with
//! diff-regurgitation dedup.

use agnt_core::conversation::turn::executor::TURN_STOPPED_NOTICE;
use agnt_core::protocol::events::Usage;
use agnt_core::transcript::entry::{MessageEntry, TranscriptEntry};
use agnt_core::transcript::ir::{ContentKind, ContentLine, MessageRole};

use super::transcript_store::TranscriptStore;
use super::{ScrollState, StatusMessageKind, StatusState, UiPhase};

/// LLM-domain decisions extracted from the former `reduce_ui_event_impl` LLM
/// arms. The streaming cursor lives in [`TranscriptStore`]
/// ([`TranscriptStore::assistant_stream_start`]) because it is an index into
/// the transcript entries and eviction must shift it on every push.
#[derive(Debug, Clone, Default)]
pub struct LlmState;

impl LlmState {
    /// Fold an `InferenceStarted` event.
    pub fn started(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        phase: &mut UiPhase,
        input_locked: &mut bool,
    ) -> bool {
        self.handle_start(store, status, phase, input_locked)
    }

    /// Fold an `InferenceCompleted` event: record the call's usage.
    pub fn completed(&mut self, status: &mut StatusState, usage: &Usage) -> bool {
        handle_llm_end(
            status,
            usage.input_tokens,
            usage.output_tokens,
            usage.total_tokens,
            usage.cost,
            usage.cost_complete,
        )
    }

    /// Fold an `InferenceChunk` event: re-render the streamed assistant block
    /// from the accumulated text the event carries.
    pub fn chunk(
        &mut self,
        store: &mut TranscriptStore,
        scroll: &mut ScrollState,
        aggregated: &str,
    ) -> bool {
        log::trace!(
            "reducer: InferenceChunk aggregated_len={}",
            aggregated.len()
        );
        self.assistant_message(store, scroll, aggregated)
    }

    /// Fold an `InferenceStopped` event.
    pub fn stopped(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        scroll: &mut ScrollState,
        reason: &str,
    ) -> bool {
        self.stopped_impl(store, status, scroll, reason)
    }

    /// Render a hook-stop reason. The generic user-cancel marker
    /// (`TURN_STOPPED_NOTICE`) becomes a brief status-bar marker; any other
    /// reason is APPENDED as a new transcript entry — never routed through the
    /// `assistant_message` dedup path, which would truncate the streamed
    /// block (Fix 2 of the repetition stop UX). The streaming cursor is
    /// cleared so the streamed block stays intact and any later assistant
    /// message starts a fresh block.
    fn stopped_impl(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        scroll: &mut ScrollState,
        reason: &str,
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
        if trimmed == TURN_STOPPED_NOTICE {
            // The user-cancel marker belongs in the status bar, not the
            // transcript: keep the streamed blocks, push nothing, and leave
            // the scroll position alone.
            store.assistant_stream_start = None;
            status.message.set_notice(TURN_STOPPED_NOTICE);
            return true;
        }
        // Close the in-progress streamed entry: push_entry's unified spacer
        // rule separates it from the notice, and the cursor resets so nothing
        // later truncates it.
        store.assistant_stream_start = None;
        scroll.scroll_transcript_to_bottom();
        store.push_entry(TranscriptEntry::Message(MessageEntry {
            role: MessageRole::Assistant,
            markdown: trimmed.to_string(),
        }));
        true
    }

    fn handle_start(
        &mut self,
        store: &mut TranscriptStore,
        status: &mut StatusState,
        phase: &mut UiPhase,
        input_locked: &mut bool,
    ) -> bool {
        if *phase == UiPhase::Idle {
            *phase = UiPhase::Busy;
            *input_locked = true;
        }
        // A Notice marker (the user-cancel stop marker) has no TTL, so the
        // next turn start is what retires it. Only Notice is cleared here: a
        // Warning keeps its own TTL and a Neutral message its own expiry.
        if status.message.kind() == StatusMessageKind::Notice {
            status.message.clear();
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

        // If this is the first delta, record where the message starts.
        // push_entry's unified spacer rule separates the streamed entry from
        // whatever precedes it (tool entry or previous turn).
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
        store.push_entry(TranscriptEntry::Message(MessageEntry {
            role: MessageRole::Assistant,
            markdown: trimmed.to_string(),
        }));
        true
    }
}

fn handle_llm_end(
    status: &mut StatusState,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: Option<u64>,
    cost: Option<f64>,
    cost_complete: bool,
) -> bool {
    status
        .tokens
        .record_token_usage(input_tokens, output_tokens, total_tokens);
    status.tokens.session_cost.record(cost, cost_complete);
    true
}

// region:    --- Support

fn assistant_diff_regurgitation_is_redundant(
    store: &TranscriptStore,
    assistant_lines: &[ContentLine],
) -> bool {
    let Some(latest_tool_display_diff) = last_tool_display_diff_lines(store) else {
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

/// The Diff-section content lines of the last tool entry's unified display.
///
/// The store is the sibling seam: the walk runs in reverse and stops at the
/// last tool entry, so an intervening message does not blind the check. Only
/// typed `ContentKind::Diff` sections contribute — the check never scans entry
/// text for diff markers. `None` when the last tool entry carries no display,
/// or when its display carries no diff section.
fn last_tool_display_diff_lines(store: &TranscriptStore) -> Option<Vec<String>> {
    let display = store.entries().iter().rev().find_map(|entry| match entry {
        TranscriptEntry::Tool(tool) => Some(tool.result.as_ref().or(tool.preview.as_ref())),
        _ => None,
    })?;
    let display = display?;

    let lines = display
        .sections
        .iter()
        .filter(|section| matches!(section.kind, ContentKind::Diff { .. }))
        .flat_map(|section| section.content.lines().map(str::to_string))
        .collect::<Vec<_>>();

    (!lines.is_empty()).then_some(lines)
}

// endregion: --- Support
