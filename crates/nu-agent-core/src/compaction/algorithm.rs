use std::future::Future;
use std::io;

use super::helpers::{estimate_tokens, find_safe_split_index};
use super::strategy::{CompactionOutcome, CompactionParams, CompactionStrategy};
use crate::session::{CompactionMarker, JournalConversationMemory};
use crate::types::{AssistantContent, Message, ToolResultContent, UserContent};
use rig::memory::ConversationMemory;

/// Compacts messages using `JournalConversationMemory`.
///
/// This function:
/// 1. Loads messages from the conversation memory (cache or JSONL on miss)
/// 2. Applies strategy-specific compaction:
///    - **SlidingSummary**: summarizes ALL messages, LLM context = `[System(summary)]` only
///    - **SlidingWindow**: keeps last N messages verbatim
///    - **TokenTruncate**: keeps newest messages within token budget
/// 3. Appends compaction marker to JSONL (durable commit point)
/// 4. For non-SlidingSummary strategies, re-appends kept messages to JSONL
/// 5. Clears in-memory cache
/// 6. Resets cache to LLM context (in-memory only — no JSONL write)
///
/// # Arguments
/// * `session_id` - The session ID to compact
/// * `config` - Compaction parameters (thresholds, strategy, budget)
/// * `memory` - `JournalConversationMemory` owning both cache and JSONL store
/// * `summarizer` - Function that takes rig messages and returns a (summary, token_count) tuple
///
/// # Returns
/// `CompactionOutcome` with counts, summary text, and summary token usage
///
/// # Errors
/// Returns an error if memory operations, summarizer, or store operations fail.
pub async fn compact<F, Fut>(
    session_id: &str,
    config: &CompactionParams,
    memory: &JournalConversationMemory,
    summarizer: F,
) -> io::Result<CompactionOutcome>
where
    F: FnOnce(&[Message]) -> Fut,
    Fut: Future<Output = io::Result<(String, Option<u64>)>>,
{
    let keep_count = config.keep_recent;

    // Load messages from memory
    let messages = memory
        .load(session_id)
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;

    // Strategy-specific: build compacted messages, summary text, and counts.
    // TokenTruncate uses token budgets on ALL messages (ignores keep_recent/split).
    // SlidingSummary summarizes ALL messages — the split is used only for the early-return guard.
    // SlidingWindow splits messages into old/recent at a fixed index.
    let (
        llm_context,
        summary_text,
        summarized_count,
        kept_recent_count,
        strategy_name,
        store_kept_messages,
        summary_total_tokens,
    ) = match config.compaction_strategy {
        CompactionStrategy::TokenTruncate => {
            let budget = config.token_budget.ok_or_else(|| {
                io::Error::other("TokenTruncate strategy requires token_budget to be set")
            })?;
            let mut kept: Vec<Message> = Vec::new();
            let mut total_tokens: usize = 0;
            for msg in messages.iter().rev() {
                let msg_tokens = estimate_tokens(msg);
                if total_tokens + msg_tokens > budget && !kept.is_empty() {
                    break;
                }
                total_tokens += msg_tokens;
                kept.push(msg.clone());
            }
            kept.reverse();
            if let Some(Message::System { .. }) = messages.first()
                && !matches!(kept.first(), Some(Message::System { .. }))
            {
                kept.insert(0, messages[0].clone());
            }
            let kept_count = kept.len();
            let dropped = messages.len().saturating_sub(kept_count);
            let store_kept = kept.clone();
            (
                kept,
                String::new(),
                dropped,
                kept_count,
                "token_truncate",
                store_kept,
                None::<u64>,
            )
        }
        _ => {
            // SlidingWindow uses keep_recent split to preserve recent messages verbatim.
            // SlidingSummary enters this branch for the early-return guard only;
            // it then summarizes ALL messages (split variables are unused).
            if messages.len() <= keep_count {
                return Ok(CompactionOutcome {
                    summarized_count: 0,
                    kept_recent_count: messages.len(),
                    summary_text: String::new(),
                    summary_total_tokens: None,
                });
            }

            // Split messages into old (to summarize) and recent (to keep).
            // Use group-aware split to avoid breaking tool call/result pairs.
            let naive_index = messages.len() - keep_count;
            let split_index = find_safe_split_index(&messages, naive_index);
            let old_messages = &messages[..split_index];
            let recent_messages = &messages[split_index..];
            let summarized_count = old_messages.len();
            let kept_recent_count = recent_messages.len();

            match config.compaction_strategy {
                CompactionStrategy::SlidingSummary => {
                    // Summarize ALL messages — recent are included in the summary.
                    let all_count = messages.len();

                    // Defense in depth: scan for tool/MCP failure patterns and inject
                    // a warning into the summarizer input so the summary preserves
                    // failure context. This prevents the LLM from retrying broken
                    // tools after compaction discards the original error messages.
                    let failures = detect_failure_patterns(&messages);
                    let extra = if failures.is_empty() {
                        None
                    } else {
                        let mut input = messages.clone();
                        input.push(Message::system(FAILURE_WARNING));
                        Some(input)
                    };
                    let summarizer_input = extra.as_deref().unwrap_or(&messages);

                    let (summary, summary_tokens) = summarizer(summarizer_input).await?;
                    let compacted = vec![Message::system(&summary)];
                    (
                        compacted,
                        summary,
                        all_count,
                        0,
                        "sliding_summary",
                        Vec::new(),
                        summary_tokens,
                    )
                }
                CompactionStrategy::SlidingWindow => {
                    let store_kept = recent_messages.to_vec();
                    (
                        store_kept.clone(),
                        String::new(),
                        summarized_count,
                        kept_recent_count,
                        "sliding_window",
                        store_kept,
                        None::<u64>,
                    )
                }
                CompactionStrategy::TokenTruncate => {
                    unreachable!("TokenTruncate handled above")
                }
            }
        }
    };

    // Append compaction marker to JSONL only (durable commit point).
    // Write None for last_total_tokens — the pre-compaction value is stale.
    // The real post-compaction count is summary_total_tokens and is set in-memory
    // at the runtime level, not written to the JSONL here.
    let marker = CompactionMarker::new(
        summary_text.clone(),
        kept_recent_count,
        summarized_count,
        strategy_name,
    );
    memory.append_marker(session_id, &marker, None)?;

    // Re-append kept messages to JSONL only (TokenTruncate and SlidingWindow).
    // SlidingSummary returns empty store_kept_messages — marker is the last entry.
    if !store_kept_messages.is_empty() {
        memory.append_messages_to_store_only(session_id, &store_kept_messages, None)?;
    }

    // Reset the in-memory cache to the compacted LLM context.
    // clear() evicts the cache entry; reset_context() refills it from the
    // already-computed llm_context without touching JSONL again.
    memory
        .clear(session_id)
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;

    // reset_context() is infallible (it's a HashMap insert). If this code path
    // is somehow skipped (defensive), a subsequent load() will re-read JSONL
    // cleanly because clear() already evicted the stale cache entry.
    memory.reset_context(session_id, llm_context);

    Ok(CompactionOutcome {
        summarized_count,
        kept_recent_count,
        summary_text,
        summary_total_tokens,
    })
}

/// Warning text injected into the summarizer input when failure patterns are detected.
/// Defense in depth — the circuit breaker (Part 1) and doom loop persistence (Part 2)
/// are the primary defenses; this ensures the summary preserves failure context.
const FAILURE_WARNING: &str = "IMPORTANT: The conversation contains tool/MCP failures. \
    Your summary MUST include a section noting which tools or MCP servers failed and why \
    (e.g., 'Transport closed', 'server disconnected'). This prevents the next turn from \
    retrying broken tools.";

/// Patterns (case-insensitive) that indicate tool or MCP failures in message content.
const FAILURE_PATTERNS: &[&str] = &[
    "transport closed",
    "transport error",
    "connection refused",
    "is not available",
    "doom loop detected",
    "[turn failed:",
    "mcp server",
];

/// Scans messages for tool/MCP failure patterns.
///
/// Returns a deduplicated list of matched patterns (lowercased). Uses the same
/// text extraction approach as `message_char_count` in `token_estimate.rs`.
pub(crate) fn detect_failure_patterns(messages: &[Message]) -> Vec<String> {
    let mut found = Vec::new();
    for msg in messages {
        let text = extract_message_text(msg);
        let lower = text.to_lowercase();
        for pattern in FAILURE_PATTERNS {
            if lower.contains(pattern) && !found.contains(&(*pattern).to_string()) {
                found.push((*pattern).to_string());
            }
        }
    }
    found
}

/// Extracts text content from a message for failure pattern matching.
///
/// Covers User (Text + ToolResult text), Assistant (Text only — ToolCall arguments
/// are request JSON, not error text), and System messages.
///
/// Mirrors the `message_char_count` pattern in `token_estimate.rs` but returns
/// concatenated text instead of a character count.
fn extract_message_text(msg: &Message) -> String {
    match msg {
        Message::User { content } => content
            .iter()
            .map(|c| match c {
                UserContent::Text(t) => t.text.clone(),
                UserContent::ToolResult(tr) => tr
                    .content
                    .iter()
                    .map(|tc| match tc {
                        ToolResultContent::Text(t) => t.text.clone(),
                        _ => String::new(),
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        Message::Assistant { content, .. } => content
            .iter()
            .map(|c| match c {
                AssistantContent::Text(t) => t.text.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        Message::System { content } => content.clone(),
    }
}
