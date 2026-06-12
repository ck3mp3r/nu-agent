use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::agent::protocol::{
    compaction::CompactionTriggerSource,
    contracts::ProgressUi,
    event::UiEvent,
};
use crate::session::{CompactionInvocationMode, CompactionOutcome, ConversationStore, Session};

pub(super) const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

const COMPACTION_SUMMARY_PROMPT: &str = include_str!("prompts/compaction_summary.md");

/// RAII guard that resets the compaction flag when dropped, even on error/panic.
pub(super) struct CompactionGuard(pub(super) Arc<AtomicBool>);

impl Drop for CompactionGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(super) fn execute_compaction_event_shared<F>(
    source: CompactionTriggerSource,
    mut execute: F,
) -> Result<UiEvent, String>
where
    F: FnMut() -> Result<Option<CompactionOutcome>, String>,
{
    let outcome = execute()?;
    let (summarized_count, kept_recent_count, summary_body) = match outcome {
        Some(outcome) => (
            outcome.summarized_count,
            outcome.kept_recent_count,
            outcome.summary_text,
        ),
        None => (
            0usize,
            0usize,
            "No-op: insufficient messages to summarize.".to_string(),
        ),
    };

    Ok(UiEvent::CompactionTriggered {
        source: source.as_str().to_string(),
        summarized_count,
        kept_recent_count,
        summary_preview: summary_preview_text(&summary_body),
        summary_body,
    })
}

/// Parameters for a single compaction invocation.
pub(super) struct CompactionInvocation<'a> {
    pub(super) mode: CompactionInvocationMode,
    pub(super) source: &'a str,
    pub(super) last_total_tokens: Option<u64>,
}

/// Execute compaction using rig memory and ConversationStore.
///
/// This async function:
/// 1. Loads messages from InMemoryConversationMemory
/// 2. Calls the summarizer with old rig messages
/// 3. Compacts using `Session::compact`
/// 4. Updates memory and persists to store
///
/// # Arguments
/// * `session` - Session to compact
/// * `memory` - InMemoryConversationMemory containing messages
/// * `store` - ConversationStore for persistence
/// * `model` - Completion model for summarization
/// * `ui` - Progress UI for emitting events
/// * `invocation` - Compaction mode, source label, and token state
///
/// # Returns
/// Ok(Some(outcome)) on successful compaction, Ok(None) if no compaction needed
pub(super) async fn execute_compaction<M, S, U>(
    session: &mut Session,
    memory: &rig::memory::InMemoryConversationMemory,
    store: &S,
    model: M,
    ui: &mut U,
    invocation: CompactionInvocation<'_>,
) -> Result<Option<CompactionOutcome>, String>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    S: ConversationStore,
    U: ProgressUi,
{
    use rig::memory::ConversationMemory;

    // Load messages from memory to check threshold
    let messages = memory
        .load(session.id())
        .await
        .map_err(|e| format!("Failed to load messages from memory: {}", e))?;

    // Determine if compaction should run
    let should_compact = match invocation.mode {
        CompactionInvocationMode::Threshold => {
            messages.len() > session.config().compaction_threshold
        }
        CompactionInvocationMode::Force => true,
    };

    if !should_compact {
        return Ok(None);
    }

    // Perform compaction with summarizer closure
    let source_owned = invocation.source.to_string();
    let summarizer = |old_messages: &[rig::completion::Message]| {
        let messages = old_messages.to_vec();
        let model_clone = model.clone();
        let src = source_owned.clone();
        async move { summarize_messages(model_clone, ui, &messages, &src).await }
    };

    let outcome = session
        .compact(memory, store, summarizer, invocation.last_total_tokens)
        .await
        .map_err(|_| COMPACTION_FAILURE_WARNING.to_string())?;

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
fn format_messages_for_summary(messages: &[rig::completion::Message]) -> String {
    use rig::completion::message::{AssistantContent, UserContent};

    messages
        .iter()
        .map(|msg| {
            let role = match msg {
                rig::completion::Message::User { .. } => "user",
                rig::completion::Message::Assistant { .. } => "assistant",
                rig::completion::Message::System { .. } => "system",
            };

            let content = match msg {
                rig::completion::Message::User { content } => {
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
                rig::completion::Message::Assistant { content, .. } => {
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
                rig::completion::Message::System { content } => content.clone(),
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
async fn summarize_messages<M, U>(
    model: M,
    ui: &mut U,
    old_messages: &[rig::completion::Message],
    source: &str,
) -> std::io::Result<String>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    U: ProgressUi,
{
    use futures::StreamExt;
    use rig::completion::Completion;

    let history = format_messages_for_summary(old_messages);
    let prompt_text = COMPACTION_SUMMARY_PROMPT.replace("{history}", &history);

    // Build rig agent from model
    let agent = rig::agent::AgentBuilder::new(model).build();

    let stream_result = agent
        .completion(&prompt_text, Vec::<rig::completion::Message>::new())
        .await
        .map_err(|e| std::io::Error::other(format!("{}", e)))?
        .tools(vec![])
        .stream()
        .await
        .map_err(|e| std::io::Error::other(format!("{}", e)))?;

    let mut stream = std::pin::pin!(stream_result);
    let mut aggregated = String::new();

    loop {
        if ui.take_cancel_requested() {
            return Err(std::io::Error::other("Compaction cancelled by user"));
        }

        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(Ok(chunk)) => {
                        if let rig::streaming::StreamedAssistantContent::Text(delta) = chunk {
                            aggregated.push_str(&delta.text);
                            ui.emit(&UiEvent::CompactionSummaryChunk {
                                source: source.to_string(),
                                delta: delta.text,
                                aggregated: aggregated.clone(),
                            });
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

    Ok(aggregated)
}

#[cfg(test)]
#[path = "compaction_test.rs"]
mod compaction_test;
