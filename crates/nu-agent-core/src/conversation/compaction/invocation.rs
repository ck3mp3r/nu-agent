use std::time::Duration;

use crate::compaction::CompactionOutcome;
use crate::protocol::{compaction::CompactionTriggerSource, contracts::ProgressUi, event::UiEvent};
use crate::session::{CachedMemory, SessionStore};
use crate::types::{AssistantContent, Message, UserContent};

pub(in crate::conversation) const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

const COMPACTION_SUMMARY_PROMPT: &str = include_str!("prompts/compaction_summary.md");

pub(in crate::conversation) async fn execute_compaction_event_shared<F, Fut>(
    source: CompactionTriggerSource,
    execute: F,
) -> Result<Option<(UiEvent, Option<u64>)>, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Option<CompactionOutcome>, String>>,
{
    let Some(outcome) = execute().await? else {
        return Ok(None);
    };

    Ok(Some((
        UiEvent::CompactionTriggered {
            source: source.as_str().to_string(),
            summarized_count: outcome.summarized_count,
            kept_recent_count: outcome.kept_recent_count,
            summary_preview: summary_preview_text(&outcome.summary_text),
            summary_body: outcome.summary_text,
        },
        outcome.summary_total_tokens,
    )))
}

/// Parameters for a single compaction invocation.
pub(in crate::conversation) struct CompactionInvocation<'a> {
    pub(in crate::conversation) source: &'a str,
}

/// Execute compaction using any `CachedMemory<S: SessionStore>` with explicit config.
///
/// This async function:
/// 1. Loads messages from the conversation memory
/// 2. Calls the summarizer with old rig messages
/// 3. Compacts using `compact()`
/// 4. Updates in-memory state and persists marker + kept messages to store
///
/// # Arguments
/// * `session_id` - The session ID to compact
/// * `params` - Compaction parameters (thresholds, strategy, budget)
/// * `memory` - `CachedMemory<S>` owning cache and backing store
/// * `model` - Completion model for summarization
/// * `ui` - Progress UI for emitting events
/// * `invocation` - Compaction mode, source label, and token state
///
/// # Returns
/// Ok(Some(outcome)) on successful compaction, Ok(None) if no compaction needed
pub(in crate::conversation) async fn execute_compaction_with_config<S, M, U>(
    session_id: &str,
    params: &crate::compaction::CompactionParams,
    memory: &CachedMemory<S>,
    model: M,
    ui: &mut U,
    invocation: CompactionInvocation<'_>,
) -> Result<Option<CompactionOutcome>, String>
where
    S: SessionStore + Clone + Send + Sync,
    S::Error: std::fmt::Display,
    M: rig::completion::CompletionModel + Clone + 'static,
    U: ProgressUi,
{
    // Perform compaction with summarizer closure
    let source_owned = invocation.source.to_string();
    let summarizer = |old_messages: &[Message]| {
        let messages = old_messages.to_vec();
        let model_clone = model.clone();
        let src = source_owned.clone();
        async move { summarize_messages(model_clone, ui, &messages, &src).await }
    };

    let outcome = crate::compaction::compact(session_id, params, memory, summarizer)
        .await
        .map_err(|e| e.to_string())?;

    if outcome.summarized_count == 0 {
        return Ok(None);
    }

    Ok(Some(outcome))
}

fn summary_preview_text(summary_body: &str) -> String {
    let one_line = summary_body.replace('\n', " ");
    one_line.chars().take(120).collect()
}

/// Format rig messages for summarization.
///
/// Extracts text content from rig::completion::Message variants:
/// - Message::User { content } -> text from UserContent::Text
/// - Message::Assistant { content } -> text from AssistantContent::Text
/// - Message::System { content } -> content string
///
/// Returns formatted string with role: content pairs.
fn format_messages_for_summary(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|msg| {
            let role = match msg {
                Message::User { .. } => "user",
                Message::Assistant { .. } => "assistant",
                Message::System { .. } => "system",
            };

            let content = match msg {
                Message::User { content } => {
                    // Extract text from OneOrMany<UserContent>
                    content
                        .iter()
                        .filter_map(|c| match c {
                            UserContent::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                Message::Assistant { content, .. } => {
                    // Extract text from OneOrMany<AssistantContent>
                    content
                        .iter()
                        .filter_map(|c| match c {
                            AssistantContent::Text(text) => Some(text.text.as_str()),
                            AssistantContent::ToolCall(_) => None,
                            AssistantContent::Reasoning(_) => None,
                            AssistantContent::Image(_) => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                Message::System { content } => content.clone(),
            };

            format!("{}: {}", role, content)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Summarize old rig messages with LLM.
///
/// Formats rig messages, creates summarization prompt, and calls rig agent completion.
/// Uses streaming API to emit progressive chunks via `UiEvent::CompactionSummaryChunk`.
///
/// Returns `(summary_text, total_tokens)` where `total_tokens` is captured from the
/// streaming `Final` variant if the provider yields usage data.
async fn summarize_messages<M, U>(
    model: M,
    ui: &mut U,
    old_messages: &[Message],
    source: &str,
) -> std::io::Result<(String, Option<u64>)>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    U: ProgressUi,
{
    use futures::StreamExt;

    let history = format_messages_for_summary(old_messages);
    let prompt_text = COMPACTION_SUMMARY_PROMPT.replace("{history}", &history);

    // Use the model's raw completion_request API (no agent, no hooks needed for compaction)
    let stream = model
        .completion_request(&prompt_text)
        .messages(Vec::<Message>::new())
        .stream()
        .await
        .map_err(|e| std::io::Error::other(format!("{}", e)))?;

    let mut stream = std::pin::pin!(stream);
    let mut aggregated = String::new();
    let mut summary_tokens: Option<u64> = None;

    loop {
        if ui.take_cancel_requested() {
            return Err(std::io::Error::other("Compaction cancelled by user"));
        }

        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(Ok(chunk)) => {
                        match chunk {
                            rig::streaming::StreamedAssistantContent::Text(delta) => {
                                aggregated.push_str(&delta.text);
                                ui.emit(&UiEvent::CompactionSummaryChunk {
                                    source: source.to_string(),
                                    delta: delta.text,
                                    aggregated: aggregated.clone(),
                                });
                            }
                            rig::streaming::StreamedAssistantContent::Final(response) => {
                                use rig::completion::GetTokenUsage;
                                summary_tokens = Some(response.token_usage().total_tokens);
                            }
                            _ => {}
                        }
                    }
                    Some(Err(_)) => {
                        return Err(std::io::Error::other(COMPACTION_FAILURE_WARNING));
                    }
                    None => {
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(80)) => {
                ui.emit(&UiEvent::Tick);
            }
        }
    }

    Ok((aggregated, summary_tokens))
}
