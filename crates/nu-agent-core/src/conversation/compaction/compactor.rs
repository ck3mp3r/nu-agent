//! A standalone summarizer service that summarizes evicted messages with the
//! LLM and emits TUI events on the bus.
//!
//! `NuCompactor` is invoked directly by the `HookChain` compaction logic (via
//! `RequestPatch::history`), not through a rig `Compactor` trait. It produces a
//! `SummaryArtifact` that converts into a `Message::User` with the required
//! "What we did thus far:" header.

use std::sync::{Arc, Mutex};

use crate::bus::{Bus, CompactionEvent};
use crate::session::{CompactionMarker, SessionStore, StoreEntry};
use crate::types::{Message, UserContent};
use chrono::Utc;

use futures::StreamExt;
use rig::agent::ModelHandle;
use rig::completion::CompletionModel;
use rig::memory::MemoryError;

/// The header every artifact body must start with.
const SUMMARY_HEADER: &str = "What we did thus far:\n\n";
/// Marker appended when the artifact body is truncated to `max_bytes`.
const TRUNCATION_MARKER: &str = "[…truncated…]";

/// Key in a `Text` content block's `additional_params` marking it as a
/// compaction summary. `merge_consecutive_same_role` in `session/repair.rs`
/// checks this key instead of the text content so a summary artifact is never
/// folded into the next `User` turn.
pub const COMPACTION_SUMMARY_KEY: &str = "compaction_summary";

const COMPACTION_SUMMARY_PROMPT: &str = include_str!("prompts/compaction_summary.md");
const COMPACTION_ROLLING_PROMPT: &str = include_str!("prompts/compaction_rolling.md");

/// The artifact produced by `NuCompactor::compact()`.
///
/// Converts into a `Message::User` whose text is the summary (with an optional
/// truncation marker when the artifact is capped to `max_bytes`).
#[derive(Debug, Clone)]
pub struct SummaryArtifact {
    summary: String,
    total_tokens: Option<u64>,
}

impl SummaryArtifact {
    /// Construct an artifact from a persisted marker summary (cached-summary
    /// path), with no token attribution.
    pub fn from_marker_summary(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            total_tokens: None,
        }
    }

    /// The summarized text, ready to be converted into a user message.
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Total tokens reported by the provider for the summarizer call, if any.
    pub fn total_tokens(&self) -> Option<u64> {
        self.total_tokens
    }
}

impl From<SummaryArtifact> for Message {
    fn from(artifact: SummaryArtifact) -> Self {
        let text = format!("{SUMMARY_HEADER}{}", artifact.summary);
        Message::User {
            content: vec![UserContent::Text(crate::types::Text {
                text,
                additional_params: crate::types::AdditionalParams::from_entries([(
                    COMPACTION_SUMMARY_KEY,
                    serde_json::json!(true),
                )]),
            })],
        }
    }
}

/// A `Compactor` that summarizes a batch of evicted messages with the LLM and
/// emits `CompactionEvent::Started`/`SummaryChunk` events on the bus during
/// streaming.
///
/// The summarizer input is `carry_over` text (if any) prepended to the formatted
/// evicted messages, wrapped in the compaction summary prompt.
///
/// The model is held as `Arc<Mutex<ModelHandle>>` so it can be swapped at
/// runtime via `set_model()` — `ModelHandle` is rig's erased, cloneable handle,
/// so the concrete `NuCompactor<S>` type stays fixed regardless of provider. The
/// handle is constructed eagerly at startup and never empty.
pub struct NuCompactor<S = NoopStore>
where
    S: SessionStore + Clone + Send + Sync,
{
    model: Arc<Mutex<ModelHandle>>,
    bus: Bus,
    max_bytes: Option<usize>,
    /// Backing store for persisting compaction markers. When set, `compact()`
    /// appends a `CompactionMarker` after producing the artifact.
    store: Option<Arc<S>>,
}

impl<S: SessionStore + Clone + Send + Sync> Clone for NuCompactor<S> {
    fn clone(&self) -> Self {
        Self {
            model: Arc::clone(&self.model),
            bus: self.bus.clone(),
            max_bytes: self.max_bytes,
            store: self.store.clone(),
        }
    }
}

/// A `SessionStore` that is never used — the default `S` type parameter so
/// `NuCompactor` can be named without an explicit store when marker
/// persistence is not needed (e.g. unit tests of the compactor itself).
#[derive(Debug, Clone)]
pub struct NoopStore;

impl SessionStore for NoopStore {
    type Error = std::io::Error;

    async fn create(&self, _id: &str, _first_messages: &[Message]) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn load(
        &self,
        _id: &str,
    ) -> Result<Option<(crate::session::SessionMetadata, Vec<StoreEntry>)>, Self::Error> {
        Ok(None)
    }

    async fn append(&self, _id: &str, _entries: &[StoreEntry]) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn replace_entries(&self, _id: &str, _entries: &[StoreEntry]) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn list(&self) -> Result<Vec<crate::session::SessionInfo>, Self::Error> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &str) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<S: SessionStore + Clone + Send + Sync> NuCompactor<S> {
    /// Construct a `NuCompactor` with no marker store.
    pub fn new(model: ModelHandle, bus: Bus, max_bytes: Option<usize>) -> Self {
        Self {
            model: Arc::new(Mutex::new(model)),
            bus,
            max_bytes,
            store: None,
        }
    }

    /// Construct a `NuCompactor` from an existing shared model handle.
    ///
    /// Unlike `new()`, which creates its own internal `Arc<Mutex<ModelHandle>>`,
    /// this takes an external `Arc` so the same handle can be shared with other
    /// consumers (e.g. `HookChain`). A `set_model()` call through the shared
    /// `Arc` from outside is visible to the next `compact()` call.
    pub fn from_shared_model(
        model_arc: Arc<Mutex<ModelHandle>>,
        bus: Bus,
        max_bytes: Option<usize>,
    ) -> Self {
        Self {
            model: model_arc,
            bus,
            max_bytes,
            store: None,
        }
    }

    /// Attach a backing store so `compact()` persists a `CompactionMarker`
    /// after producing the artifact.
    pub fn with_store(mut self, store: Arc<S>) -> Self {
        self.store = Some(store);
        self
    }

    /// Swap the model used by subsequent `compact()` calls.
    ///
    /// Locks the shared inner handle and replaces it. Any in-flight `compact()`
    /// call keeps the handle clone it already took, so it finishes with the old
    /// model; the next call uses the new one.
    pub fn set_model(&self, handle: ModelHandle) {
        *self.model.lock().expect("model mutex poisoned") = handle;
    }

    /// Summarize `evicted` messages and return the artifact.
    ///
    /// `carry_over` (a previous artifact) is prepended to the summarizer input to
    /// preserve rolling context across compactions.
    pub async fn compact(
        &self,
        conversation_id: &str,
        evicted: &[Message],
        carry_over: Option<&SummaryArtifact>,
        source: &str,
    ) -> Result<SummaryArtifact, MemoryError> {
        // Load any persisted marker once, to use its summary as carry-over for
        // the rolling prompt. On restart `CompactingMemory` re-evicts the same
        // prefix (its in-process `absorbed` watermark is 0), so the marker's
        // summary keeps the rolling context across sessions.
        let last_marker = self.last_marker(conversation_id, source).await?;

        // Announce start before the slow summarizer LLM call so the status
        // spinner turns on immediately.
        let _ = self
            .bus
            .compaction()
            .send(CompactionEvent::Started {
                source: source.to_string(),
            })
            .await;

        let input = self.format_messages(evicted);
        // The in-process `carry_over` is empty after a restart, but the store
        // may hold the previous summary from an earlier session. Use it so the
        // rolling summary incorporates the prior compaction.
        let carry_text = match carry_over {
            Some(artifact) => Some(artifact.summary()),
            None => last_marker.as_ref().map(|marker| marker.summary.as_str()),
        };
        let prompt_text = match carry_text {
            Some(summary) => COMPACTION_ROLLING_PROMPT
                .replace("{prior_summary}", summary)
                .replace("{history}", &input),
            None => COMPACTION_SUMMARY_PROMPT.replace("{history}", &input),
        };

        // Lock the shared handle and clone it so the LLM call runs against a
        // stable model; a concurrent `set_model()` then only affects the next
        // call. The handle is constructed eagerly at startup, so it is always
        // present here.
        let model = self.model.lock().expect("model mutex poisoned").clone();

        let stream = match model
            .completion_request(&prompt_text)
            .messages(Vec::<Message>::new())
            .stream()
            .await
        {
            Ok(stream) => stream,
            Err(e) => {
                let _ = self
                    .bus
                    .compaction()
                    .send(CompactionEvent::Failed {
                        source: source.to_string(),
                        message: e.to_string(),
                    })
                    .await;
                return Err(MemoryError::Backend(e.to_string().into()));
            }
        };

        let mut stream = std::pin::pin!(stream);
        let mut aggregated = String::new();
        let mut total_tokens: Option<u64> = None;
        let mut cancel_rx = self.bus.cancel().subscribe();

        loop {
            tokio::select! {
                item = stream.next() => {
                    match item {
                        Some(Ok(chunk)) => match chunk {
                            rig::streaming::StreamedAssistantContent::Text(delta) => {
                                aggregated.push_str(&delta.text);
                                let _ = self.bus.compaction().send(CompactionEvent::SummaryChunk {
                                    source: source.to_string(),
                                    delta: delta.text,
                                    aggregated: aggregated.clone(),
                                }).await;
                            }
                            rig::streaming::StreamedAssistantContent::Reasoning { .. }
                            | rig::streaming::StreamedAssistantContent::ReasoningDelta { .. } => {
                                // Reasoning is not summary content. Log it for
                                // observability but do not append it to the summary.
                                log::debug!(
                                    "Compaction stream produced reasoning chunk (not summary content): {chunk:?}"
                                );
                            }
                            rig::streaming::StreamedAssistantContent::Final(response) => {
                                total_tokens = Some(response.usage.total_tokens);
                            }
                            rig::streaming::StreamedAssistantContent::ToolCall { .. } => {
                                let _ = self.bus.compaction().send(CompactionEvent::Failed {
                                    source: source.to_string(),
                                    message: "Unexpected tool call during compaction".to_string(),
                                }).await;
                                return Err(MemoryError::Backend(
                                    "Unexpected tool call during compaction".to_string().into(),
                                ));
                            }
                            rig::streaming::StreamedAssistantContent::ToolCallDelta { .. } => {
                                let _ = self.bus.compaction().send(CompactionEvent::Failed {
                                    source: source.to_string(),
                                    message: "Unexpected tool call delta during compaction"
                                        .to_string(),
                                }).await;
                                return Err(MemoryError::Backend(
                                    "Unexpected tool call delta during compaction"
                                        .to_string()
                                        .into(),
                                ));
                            }
                            rig::streaming::StreamedAssistantContent::Unknown(_) => {
                                let _ = self.bus.compaction().send(CompactionEvent::Failed {
                                    source: source.to_string(),
                                    message: "Unexpected stream chunk: unknown".to_string(),
                                }).await;
                                return Err(MemoryError::Backend(
                                    "Unexpected stream chunk: unknown".to_string().into(),
                                ));
                            }
                        },
                        Some(Err(e)) => {
                            let message = format!("Compaction stream error: {e}");
                            let _ = self.bus.compaction().send(CompactionEvent::Failed {
                                source: source.to_string(),
                                message: message.clone(),
                            }).await;
                            return Err(MemoryError::Backend(message.into()));
                        }
                        None => break,
                    }
                }
                Ok(_) = cancel_rx.recv() => {
                    let _ = self.bus.compaction().send(CompactionEvent::Failed {
                        source: source.to_string(),
                        message: "Compaction cancelled by user".to_string(),
                    }).await;
                    return Err(MemoryError::Backend(
                        "Compaction cancelled by user".to_string().into(),
                    ));
                }
            }
        }

        // Reject an empty summary before writing a marker. A compaction that
        // produced no text (e.g. the model emitted only reasoning) must not
        // persist an empty marker nor report completion.
        if aggregated.is_empty() {
            let _ = self
                .bus
                .compaction()
                .send(CompactionEvent::Failed {
                    source: source.to_string(),
                    message: "Compaction produced an empty summary".to_string(),
                })
                .await;
            return Err(MemoryError::Backend(
                "Compaction produced an empty summary".to_string().into(),
            ));
        }

        let summary = self.apply_cap(aggregated);
        let artifact = SummaryArtifact {
            summary,
            total_tokens,
        };

        // Persist a compaction marker so a later process restart can seed the
        // cache from the store instead of re-compacting the same prefix.
        if let Some(store) = &self.store {
            let marker = CompactionMarker::new(artifact.summary.clone(), Utc::now());
            if let Err(e) = store
                .append(conversation_id, &[StoreEntry::Marker(marker)])
                .await
            {
                let _ = self
                    .bus
                    .compaction()
                    .send(CompactionEvent::Failed {
                        source: source.to_string(),
                        message: e.to_string(),
                    })
                    .await;
                return Err(MemoryError::Backend(e.to_string().into()));
            }
        }

        self.emit_completed(&artifact, source).await;

        Ok(artifact)
    }

    /// Load the last `CompactionMarker` persisted for `conversation_id`, if any.
    ///
    /// Returns `None` when no store is attached, the store returns no entries,
    /// or no marker is present among the entries.
    pub async fn last_marker(
        &self,
        conversation_id: &str,
        source: &str,
    ) -> Result<Option<CompactionMarker>, MemoryError> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let loaded = store.load(conversation_id).await;
        let loaded = match loaded {
            Ok(v) => v,
            Err(e) => {
                let _ = self
                    .bus
                    .compaction()
                    .send(CompactionEvent::Failed {
                        source: source.to_string(),
                        message: e.to_string(),
                    })
                    .await;
                return Err(MemoryError::Backend(e.to_string().into()));
            }
        };
        let Some((_, entries)) = loaded else {
            return Ok(None);
        };
        Ok(entries.iter().rev().find_map(|entry| match entry {
            StoreEntry::Marker(marker) => Some(marker.clone()),
            StoreEntry::Message(_) => None,
        }))
    }

    /// Emit `CompactionEvent::Completed` for `artifact`. Used on both the LLM
    /// path and the cached-summary deduplication path so the TUI shows the
    /// restored summary and clears any spinner.
    async fn emit_completed(&self, artifact: &SummaryArtifact, source: &str) {
        let preview: String = artifact.summary.chars().take(200).collect();
        let _ = self
            .bus
            .compaction()
            .send(CompactionEvent::Completed {
                source: source.to_string(),
                summary_preview: preview,
                summary_body: artifact.summary.clone(),
            })
            .await;
    }

    /// Format a message batch into the text fed to the summarizer prompt.
    ///
    /// Includes textual representations of every content type so the structured
    /// summary can capture tool calls, tool results, reasoning, and images.
    fn format_messages(&self, messages: &[Message]) -> String {
        messages
            .iter()
            .map(|msg| {
                let role = match msg {
                    Message::User { .. } => "user",
                    Message::Assistant { .. } => "assistant",
                    Message::System { .. } => "system",
                };
                let content = match msg {
                    Message::User { content } => content
                        .iter()
                        .map(|c| match c {
                            UserContent::Text(t) => t.text.clone(),
                            UserContent::ToolResult(tr) => {
                                let result_text: String = tr
                                    .content
                                    .iter()
                                    .filter_map(|tc| match tc {
                                        crate::types::ToolResultContent::Text(t) => {
                                            Some(t.text.as_str())
                                        }
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                format!("[Tool result]: {}", truncate(&result_text, 2000))
                            }
                            UserContent::Image(_) => "[Attached image]".to_string(),
                            UserContent::Audio(_) => "[Attached audio]".to_string(),
                            UserContent::Video(_) => "[Attached video]".to_string(),
                            UserContent::Document(_) => "[Attached document]".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                    Message::Assistant { content, .. } => content
                        .iter()
                        .map(|c| match c {
                            crate::types::AssistantContent::Text(t) => t.text.clone(),
                            crate::types::AssistantContent::ToolCall(tc) => {
                                let args = serde_json::to_string(&tc.function.arguments)
                                    .unwrap_or_default();
                                format!("[Assistant tool call]: {}({})", tc.function.name, args)
                            }
                            crate::types::AssistantContent::Reasoning(r) => {
                                format!("[Assistant reasoning]: {:?}", r.content)
                            }
                            crate::types::AssistantContent::Image(_) => {
                                "[Attached image]".to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                    Message::System { content } => content.clone(),
                };
                format!("{role}: {content}")
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Apply the optional `max_bytes` cap, truncating the body at a UTF-8
    /// boundary and appending the truncation marker when truncated.
    fn apply_cap(&self, summary: String) -> String {
        let Some(max) = self.max_bytes else {
            return summary;
        };
        let header_len = SUMMARY_HEADER.len();
        let marker_len = TRUNCATION_MARKER.len();

        // If the full artifact fits, no truncation is needed.
        if header_len + summary.len() <= max {
            return summary;
        }

        let body_budget = max.saturating_sub(header_len + marker_len);

        // Truncate the body to `body_budget` bytes at a UTF-8 char boundary.
        let boundary = summary
            .char_indices()
            .take_while(|(idx, _)| *idx <= body_budget)
            .map(|(idx, _)| idx)
            .last()
            .unwrap_or(0);
        let truncated_body = &summary[..boundary];
        format!("{truncated_body}{TRUNCATION_MARKER}")
    }
}

/// Truncate `text` to at most `max_chars` characters at a UTF-8 char boundary,
/// appending an ellipsis when truncated. Used to bound tool-result text fed to
/// the summarizer so a single oversized result cannot blow up the prompt.
fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…")
}
