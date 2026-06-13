use std::future::Future;
use std::io;

use super::helpers::{estimate_tokens, find_safe_split_index};
use super::strategy::{CompactionOutcome, CompactionParams, CompactionStrategy};
use crate::session::{extract_llm_context, CompactionMarker, ConversationStore};

/// Compacts messages using rig memory and ConversationStore.
///
/// This function:
/// 1. Loads messages from InMemoryConversationMemory
/// 2. Splits at `len - keep_recent`
/// 3. Formats old messages for summarization
/// 4. Calls summarizer with old messages
/// 5. Builds compacted list: [Message::system(summary)] + recent
/// 6. Appends compaction marker to ConversationStore (durable commit point)
/// 7. Clears memory and appends compacted messages (with rollback on failure)
///
/// Note: The caller is responsible for incrementing its own compaction_count.
///
/// # Arguments
/// * `session_id` - The session ID to compact
/// * `config` - Compaction parameters (thresholds, strategy, budget)
/// * `memory` - InMemoryConversationMemory containing session messages
/// * `store` - ConversationStore for persistent JSONL storage
/// * `summarizer` - Function that takes rig messages and returns a summary string
/// * `last_total_tokens` - Optional token count from last LLM response
///
/// # Returns
/// CompactionOutcome with counts and summary text
///
/// # Errors
/// Returns an error if memory operations, summarizer, or store operations fail.
pub async fn compact<F, Fut, S>(
    session_id: &str,
    config: &CompactionParams,
    memory: &rig::memory::InMemoryConversationMemory,
    store: &S,
    summarizer: F,
    last_total_tokens: Option<u64>,
) -> io::Result<CompactionOutcome>
where
    F: FnOnce(&[rig::completion::Message]) -> Fut,
    Fut: Future<Output = io::Result<String>>,
    S: ConversationStore,
{
    use rig::memory::ConversationMemory;

    let keep_count = config.keep_recent;

    // Load messages from memory
    let messages = memory
        .load(session_id)
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;

    // Strategy-specific: build compacted messages, summary text, and counts.
    // TokenTruncate uses token budgets on ALL messages (ignores keep_recent/split).
    // SlidingSummary and SlidingWindow split messages into old/recent at a fixed index.
    let (
        llm_context,
        summary_text,
        summarized_count,
        kept_recent_count,
        strategy_name,
        store_kept_messages,
    ) = match config.compaction_strategy {
        CompactionStrategy::TokenTruncate => {
            let budget = config
                .token_budget
                .unwrap_or(config.compaction_threshold * 100);
            let mut kept: Vec<rig::completion::Message> = Vec::new();
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
            if let Some(rig::completion::Message::System { .. }) = messages.first()
                && !matches!(kept.first(), Some(rig::completion::Message::System { .. }))
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
            )
        }
        _ => {
            // For SlidingSummary and SlidingWindow, use keep_recent split
            if messages.len() <= keep_count {
                return Ok(CompactionOutcome {
                    summarized_count: 0,
                    kept_recent_count: messages.len(),
                    summary_text: String::new(),
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
                    let summary = summarizer(old_messages).await?;
                    let summary_message = rig::completion::Message::system(&summary);
                    let mut compacted = vec![summary_message];
                    compacted.extend_from_slice(recent_messages);
                    let store_kept = recent_messages.to_vec();
                    (
                        compacted,
                        summary,
                        summarized_count,
                        kept_recent_count,
                        "sliding_summary",
                        store_kept,
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
                    )
                }
                CompactionStrategy::TokenTruncate => {
                    unreachable!("TokenTruncate handled above")
                }
            }
        }
    };

    // Append compaction marker to store
    let marker = CompactionMarker::new(
        summary_text.clone(),
        kept_recent_count,
        summarized_count,
        strategy_name,
    );
    store
        .append_marker(session_id, &marker, last_total_tokens)
        .map_err(|e| io::Error::other(e.to_string()))?;

    // Re-append kept messages after the marker so they appear below it in transcript
    if !store_kept_messages.is_empty() {
        store
            .append(session_id, &store_kept_messages, last_total_tokens)
            .map_err(|e| io::Error::other(e.to_string()))?;
    }

    // Now update in-memory state
    memory
        .clear(session_id)
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;

    // Rollback: if append fails after clear, reload LLM context from store
    if let Err(e) = memory.append(session_id, llm_context).await {
        if let Ok((entries, _)) = store.load_all(session_id) {
            let context = extract_llm_context(&entries);
            let _ = memory.append(session_id, context).await;
        }
        return Err(io::Error::other(e.to_string()));
    }

    Ok(CompactionOutcome {
        summarized_count,
        kept_recent_count,
        summary_text,
    })
}
