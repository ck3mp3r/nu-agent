use std::time::Duration;

use crate::compaction::{CompactionInvocationMode, CompactionOutcome};
use crate::protocol::{compaction::CompactionTriggerSource, contracts::ProgressUi, event::UiEvent};
use crate::session::{JournalConversationMemory, Session};
use crate::types::{AssistantContent, Message, UserContent};
use rig::memory::ConversationMemory;

pub(in crate::conversation) const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

const COMPACTION_SUMMARY_PROMPT: &str = include_str!("prompts/compaction_summary.md");

pub(in crate::conversation) fn execute_compaction_event_shared<F>(
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
pub(in crate::conversation) struct CompactionInvocation<'a> {
    pub(in crate::conversation) mode: CompactionInvocationMode,
    pub(in crate::conversation) source: &'a str,
    pub(in crate::conversation) last_total_tokens: Option<u64>,
}

/// Execute compaction using `JournalConversationMemory`.
///
/// This async function:
/// 1. Loads messages from the conversation memory
/// 2. Calls the summarizer with old rig messages
/// 3. Compacts using `compact()`
/// 4. Updates in-memory state and persists marker + kept messages to JSONL
///
/// # Arguments
/// * `session` - Session to compact
/// * `memory` - `JournalConversationMemory` owning cache and JSONL store
/// * `model` - Completion model for summarization
/// * `ui` - Progress UI for emitting events
/// * `invocation` - Compaction mode, source label, and token state
///
/// # Returns
/// Ok(Some(outcome)) on successful compaction, Ok(None) if no compaction needed
pub(in crate::conversation) async fn execute_compaction<M, U>(
    session: &mut Session,
    memory: &JournalConversationMemory,
    model: M,
    ui: &mut U,
    invocation: CompactionInvocation<'_>,
) -> Result<Option<CompactionOutcome>, String>
where
    M: rig::completion::CompletionModel + Clone + 'static,
    U: ProgressUi,
{
    // Load messages from memory to check threshold
    let messages = memory
        .load(session.id())
        .await
        .map_err(|e| format!("Failed to load messages from memory: {}", e))?;

    // Determine if compaction should run
    let should_compact = match invocation.mode {
        CompactionInvocationMode::Threshold => {
            messages.len() > session.compaction_config().compaction_threshold
        }
        CompactionInvocationMode::Force => true,
    };

    if !should_compact {
        return Ok(None);
    }

    // Perform compaction with summarizer closure
    let source_owned = invocation.source.to_string();
    let summarizer = |old_messages: &[Message]| {
        let messages = old_messages.to_vec();
        let model_clone = model.clone();
        let src = source_owned.clone();
        async move { summarize_messages(model_clone, ui, &messages, &src).await }
    };

    let outcome = crate::compaction::compact(
        session.id(),
        session.compaction_config(),
        memory,
        summarizer,
        invocation.last_total_tokens,
    )
    .await
    .map_err(|_| COMPACTION_FAILURE_WARNING.to_string())?;

    session.increment_compaction_count();

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
async fn summarize_messages<M, U>(
    model: M,
    ui: &mut U,
    old_messages: &[Message],
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
        .completion(&prompt_text, Vec::<Message>::new())
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
