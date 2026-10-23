//! The compactor: summarizes a conversation's older messages with the LLM and
//! splices the summary in through rig-memory's compaction machinery.
//!
//! `NuCompactor` is invoked directly by the `HookChain` compaction logic. It
//! owns the `CompactionPolicy` and the `ModelSpec` the policy plans against, and
//! it emits TUI events on the bus while the summary streams.

use std::sync::{Arc, Mutex};

use crate::bus::{Bus, CompactionEvent};
use crate::config::{Config, defaults};
use crate::session::MemoryError;
use crate::types::Message;

use futures::StreamExt;
use rig::DynModel;
use rig::catalog::ModelSpec;
use rig::operation::Completion;
use rig::providers::registry::{Format, ProviderId};
use rig_memory::{CompactReason, CompactionPolicy, Summarizer, SummaryPlan, summary_author};

/// The leading text of every compaction summary message.
///
/// `merge_consecutive_same_role` in `session/repair.rs` checks this prefix so a
/// summary is never folded into the next `User` turn. It matches the text
/// `rig_memory::SummaryState::message()` writes.
pub(crate) const SUMMARY_PREFIX: &str =
    "The conversation before this point was compacted into this summary:";

/// What a compaction that ran left behind.
#[derive(Debug, Clone)]
pub struct CompactionOutcome {
    /// The conversation after the splice, as rig holds it.
    pub messages: Vec<Message>,
}

/// The `ModelSpec` the compaction policy plans against, resolved from our
/// config.
///
/// `provider` is the provider's rig impl name — the vendor the wire reports
/// (`ollama`, `openai`), not the config block's display name. The caller maps a
/// custom config key through the block's declared impl first. The name is
/// mapped to a rig `ProviderId` once, here: a name this build registers, else a
/// catalog-only id. A name that resolves to neither disables compaction for the
/// session — the spec is never mislabelled with an arbitrary registered
/// provider.
pub fn resolve_model_spec(
    provider: &str,
    model: &str,
    context_window: Option<u32>,
    max_output_tokens: Option<u32>,
) -> Option<ModelSpec> {
    let Some(id) =
        ProviderId::new(provider, Format::OpenAi).or_else(|| ProviderId::catalog(provider))
    else {
        log::warn!(
            "Compaction disabled: provider '{provider}' resolves to no registered or catalog \
             rig provider id"
        );
        return None;
    };
    let mut spec = ModelSpec::new(id, model);
    if let Some(tokens) = context_window {
        spec = spec.with_context_window(tokens);
    }
    if let Some(tokens) = max_output_tokens {
        spec = spec.with_max_output_tokens(tokens);
    }
    Some(spec)
}

/// The `ModelSpec` a resolved config's compaction policy plans against.
///
/// The one-call form of [`resolve_model_spec`]: it maps the config's provider
/// key through its declared rig impl — the vendor the wire reports — and sizes
/// the window from the config's own limits, so a call site cannot forget the
/// mapping.
pub fn resolve_model_spec_for(config: &Config) -> Option<ModelSpec> {
    let vendor = crate::conversation::providers::resolve_provider_type(
        &config.provider,
        config.provider_impl.as_deref(),
    );
    let context_window = config
        .model_context_tokens
        .or_else(|| config.max_context_tokens.map(|t| t as usize))
        .unwrap_or(defaults::MAX_CONTEXT_TOKENS as usize);
    resolve_model_spec(
        vendor,
        &config.model,
        u32::try_from(context_window).ok(),
        config.max_output_tokens,
    )
}

/// A compactor that summarizes a conversation's older messages with the LLM and
/// splices the summary in through rig-memory.
///
/// The model is held as `Arc<Mutex<DynModel<Completion>>>` so it can be swapped
/// at runtime via `set_model()` — `DynModel` is rig's erased, cloneable model
/// handle, so the concrete type stays fixed regardless of provider. The model is
/// constructed eagerly at startup and never empty.
///
/// A `None` spec means the configured provider resolved to no rig provider id:
/// compaction is disabled for the session, and `compact()` emits
/// `CompactionEvent::Failed` and returns `Err` rather than reporting a silent
/// success.
pub struct NuCompactor {
    model: Arc<Mutex<DynModel<Completion>>>,
    bus: Bus,
    policy: CompactionPolicy,
    spec: Option<ModelSpec>,
}

impl Clone for NuCompactor {
    fn clone(&self) -> Self {
        Self {
            model: Arc::clone(&self.model),
            bus: self.bus.clone(),
            policy: self.policy.clone(),
            spec: self.spec.clone(),
        }
    }
}

impl NuCompactor {
    /// Construct a `NuCompactor` that owns its model.
    pub fn new(
        model: DynModel<Completion>,
        bus: Bus,
        policy: CompactionPolicy,
        spec: Option<ModelSpec>,
    ) -> Self {
        Self {
            model: Arc::new(Mutex::new(model)),
            bus,
            policy,
            spec,
        }
    }

    /// Construct a `NuCompactor` from an existing shared model.
    ///
    /// Unlike `new()`, which creates its own internal `Arc<Mutex<DynModel<Completion>>>`,
    /// this takes an external `Arc` so the same model can be shared with other
    /// consumers (e.g. `HookChain`). A `set_model()` call through the shared
    /// `Arc` from outside is visible to the next `compact()` call.
    pub fn from_shared_model(
        model_arc: Arc<Mutex<DynModel<Completion>>>,
        bus: Bus,
        policy: CompactionPolicy,
        spec: Option<ModelSpec>,
    ) -> Self {
        Self {
            model: model_arc,
            bus,
            policy,
            spec,
        }
    }

    /// Swap the model used by subsequent `compact()` calls.
    ///
    /// Locks the shared inner model and replaces it. Any in-flight `compact()`
    /// call keeps the model clone it already took, so it finishes with the old
    /// model; the next call uses the new one.
    pub fn set_model(&self, model: DynModel<Completion>) {
        *self.model.lock().expect("model mutex poisoned") = model;
    }

    /// The policy this compactor plans with.
    pub fn policy(&self) -> &CompactionPolicy {
        &self.policy
    }

    /// The spec this compactor plans against, `None` when compaction is
    /// disabled for the session.
    pub fn spec(&self) -> Option<&ModelSpec> {
        self.spec.as_ref()
    }

    /// The model this compactor summarizes with, cloned from the shared slot.
    ///
    /// Not the spec [`Self::spec`] returns: that one is built from our config
    /// and carries no pricing. This model's own `spec()` is the catalog-bound
    /// one, priced.
    pub fn model(&self) -> DynModel<Completion> {
        self.model.lock().expect("model mutex poisoned").clone()
    }

    /// Compact `conversation_id` in `store` for `reason`.
    ///
    /// Loads the conversation, clears old tool outputs per the policy, plans the
    /// summary, streams it, splices it in through `apply_summary`, and saves.
    /// Returns `None` when nothing would be summarized.
    ///
    /// # Errors
    ///
    /// When compaction is disabled for the session, the store cannot be read or
    /// written, or the summary cannot be produced. Every failure is also emitted
    /// as `CompactionEvent::Failed`.
    pub async fn compact(
        &self,
        store: &dyn rig_memory::Store,
        conversation_id: &str,
        reason: &CompactReason,
        source: &str,
    ) -> Result<Option<CompactionOutcome>, MemoryError> {
        let Some(spec) = self.spec.as_ref() else {
            let message =
                "Compaction is disabled: the session's provider resolved to no rig provider id"
                    .to_string();
            self.emit_failed(source, &message).await;
            return Err(MemoryError::backend(message));
        };

        let mut conversation = match rig_memory::load(store, conversation_id) {
            Ok(conversation) => conversation,
            Err(e) => {
                let message = format!("Compaction load error: {e}");
                self.emit_failed(source, &message).await;
                return Err(MemoryError::backend(message));
            }
        };

        // Old tool outputs are cleared before the summary is asked for, so the
        // summarizer reads the freed context rather than the raw outputs.
        let cleared = self
            .policy
            .clearing
            .clear_conversation(&mut conversation, summary_author());
        if cleared.results > 0 {
            self.save(store, conversation_id, &mut conversation, source)
                .await?;
        }

        let state = rig_memory::summary_state(&conversation);
        let plan = match self
            .policy
            .plan_summary(&conversation, &state, reason, spec)
        {
            Ok(Some(plan)) => plan,
            Ok(None) => return Ok(None),
            Err(e) => {
                let message = format!("Compaction plan error: {e}");
                self.emit_failed(source, &message).await;
                return Err(MemoryError::backend(message));
            }
        };

        // Announce start before the slow summarizer LLM call so the status
        // spinner turns on immediately.
        let _ = self
            .bus
            .compaction()
            .send(CompactionEvent::Started {
                source: source.to_string(),
            })
            .await;

        let summary = self.stream_summary(&plan, source).await?;

        let mut next_state = plan.state;
        next_state.summary = summary;
        if rig_memory::apply_summary(&mut conversation, plan.keep_from, &next_state).is_none() {
            let message = "Compaction summary could not be spliced in".to_string();
            self.emit_failed(source, &message).await;
            return Err(MemoryError::backend(message));
        }
        self.save(store, conversation_id, &mut conversation, source)
            .await?;

        let messages = conversation
            .messages()
            .iter()
            .map(|message| (**message).clone())
            .collect();
        self.emit_completed(&next_state.summary, source).await;
        Ok(Some(CompactionOutcome { messages }))
    }

    /// Stream the summary `plan` asks for and return its validated text.
    ///
    /// Emits `SummaryChunk` per text delta and `Failed` on every rejection.
    async fn stream_summary(
        &self,
        plan: &SummaryPlan,
        source: &str,
    ) -> Result<String, MemoryError> {
        // Lock the shared model and clone it so the LLM call runs against a
        // stable model; a concurrent `set_model()` then only affects the next
        // call. The model is constructed eagerly at startup, so it is always
        // present here.
        let model = self.model.lock().expect("model mutex poisoned").clone();

        // `DynModel::stream` opens the reply synchronously; a failure to open
        // (transport, auth, malformed request) surfaces here rather than as a
        // stream item.
        let stream = match model.stream(plan.request.clone()) {
            Ok(stream) => stream,
            Err(e) => {
                let message = format!("Compaction stream error: {e}");
                self.emit_failed(source, &message).await;
                return Err(MemoryError::backend(message));
            }
        };

        let mut stream = std::pin::pin!(stream);
        let mut aggregated = String::new();
        let mut cancel_rx = self.bus.cancel().subscribe();

        loop {
            tokio::select! {
                item = stream.next() => {
                    match item {
                        Some(Ok(chunk)) => match chunk {
                            rig::streaming::Item::Event(rig::streaming::StreamEvent::Text {
                                text,
                                ..
                            }) => {
                                aggregated.push_str(&text);
                                let _ = self.bus.compaction().send(CompactionEvent::SummaryChunk {
                                    source: source.to_string(),
                                    delta: text,
                                    aggregated: aggregated.clone(),
                                }).await;
                            }
                            rig::streaming::Item::Event(rig::streaming::StreamEvent::Reasoning {
                                ..
                            }) => {
                                // Reasoning is not summary content. Log it for
                                // observability but do not append it to the summary.
                                log::debug!(
                                    "Compaction stream produced reasoning chunk (not summary content): {chunk:?}"
                                );
                            }
                            rig::streaming::Item::Event(rig::streaming::StreamEvent::Start {
                                ..
                            }) => {
                                log::trace!("Compaction stream part started: {chunk:?}");
                            }
                            rig::streaming::Item::Event(rig::streaming::StreamEvent::End {
                                content: crate::types::AssistantContent::ToolCall(_),
                                ..
                            })
                            | rig::streaming::Item::Event(
                                rig::streaming::StreamEvent::Arguments { .. },
                            ) => {
                                let message = "Unexpected tool call during compaction".to_string();
                                self.emit_failed(source, &message).await;
                                return Err(MemoryError::backend(message));
                            }
                            rig::streaming::Item::Event(rig::streaming::StreamEvent::End {
                                ..
                            }) => {
                                // The finalized part content is not summary text.
                                // Provider usage is read from the stream after the loop.
                            }
                            rig::streaming::Item::Unknown(_) => {
                                let message = "Unexpected stream chunk: unknown".to_string();
                                self.emit_failed(source, &message).await;
                                return Err(MemoryError::backend(message));
                            }
                        },
                        Some(Err(e)) => {
                            let message = format!("Compaction stream error: {e}");
                            self.emit_failed(source, &message).await;
                            return Err(MemoryError::backend(message));
                        }
                        None => break,
                    }
                }
                Ok(_) = cancel_rx.recv() => {
                    let message = "Compaction cancelled by user".to_string();
                    self.emit_failed(source, &message).await;
                    return Err(MemoryError::backend(message));
                }
            }
        }

        // The provider's usage arrives on the folded response, which `partial()`
        // exposes once the stream ended. `summary_text` validates the folded
        // answer: a truncated, tool-calling, or empty summary is refused.
        match Summarizer::summary_text(&stream.partial()) {
            Ok(text) => Ok(text),
            Err(e) => {
                let message = format!("Compaction produced an unusable summary: {e}");
                self.emit_failed(source, &message).await;
                Err(MemoryError::backend(message))
            }
        }
    }

    /// Save `conversation` to `store`, emitting `Failed` when the write fails.
    async fn save(
        &self,
        store: &dyn rig_memory::Store,
        conversation_id: &str,
        conversation: &mut rig::conversation::Conversation,
        source: &str,
    ) -> Result<(), MemoryError> {
        match rig_memory::save(store, conversation_id, conversation) {
            Ok(()) => Ok(()),
            Err(e) => {
                let message = format!("Compaction save error: {e}");
                self.emit_failed(source, &message).await;
                Err(MemoryError::backend(message))
            }
        }
    }

    /// Emit `CompactionEvent::Completed` for `summary`.
    async fn emit_completed(&self, summary: &str, source: &str) {
        let preview: String = summary.chars().take(200).collect();
        let _ = self
            .bus
            .compaction()
            .send(CompactionEvent::Completed {
                source: source.to_string(),
                summary_preview: preview,
                summary_body: summary.to_string(),
            })
            .await;
    }

    /// Emit `CompactionEvent::Failed` carrying `message`.
    async fn emit_failed(&self, source: &str, message: &str) {
        let _ = self
            .bus
            .compaction()
            .send(CompactionEvent::Failed {
                source: source.to_string(),
                message: message.to_string(),
            })
            .await;
    }
}
