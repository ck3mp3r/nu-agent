//! The compactor: summarizes a conversation's older messages with the LLM and
//! splices the summary in through rig-memory's compaction machinery.
//!
//! `NuCompactor` is invoked directly by the `HookChain` compaction logic. It
//! owns the `CompactionPolicy` and the `ModelSpec` the policy plans against, and
//! it emits TUI events on the bus while the summary streams.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::bus::Bus;
use crate::config::{Config, PluginConfig, defaults};
use crate::session::MemoryError;
use crate::types::Message;

use agnt_kernel::CompactionSource;

use rig::DynModel;
use rig::catalog::{Catalog, ModelSpec};
use rig::completion::CompletionRequest;
use rig::operation::Completion;
use rig::providers::registry::{Format, ProviderId};
use rig_memory::{CompactReason, CompactionPolicy, SummaryPlan, summary_author};

use super::engine::{self, Chunks};

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
///
/// `max_input_tokens` is the most one request may send, when a row lists it
/// apart from the window; rig's `input_limit()` reads it before the window.
pub fn resolve_model_spec(
    provider: &str,
    model: &str,
    context_window: Option<u32>,
    max_output_tokens: Option<u32>,
    max_input_tokens: Option<u32>,
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
    if let Some(tokens) = max_input_tokens {
        spec = spec.with_max_input_tokens(tokens);
    }
    Some(spec)
}

/// The `ModelSpec` a resolved config's compaction policy plans against.
///
/// The one-call form of [`resolve_model_spec`]: it maps the config's provider
/// key through its declared rig impl — the vendor the wire reports — and sizes
/// the window from the config's own limits, so a call site cannot forget the
/// mapping. The input limit comes from `plugin_config`'s models cache, through
/// `input_limit_for`.
pub fn resolve_model_spec_for(
    config: &Config,
    plugin_config: Option<&PluginConfig>,
) -> Option<ModelSpec> {
    let vendor = config.provider_impl.as_deref().unwrap_or(&config.provider);
    let context_window = config
        .model_context_tokens
        .or_else(|| config.max_context_tokens.map(|t| t as usize))
        .unwrap_or(defaults::MAX_CONTEXT_TOKENS as usize);
    resolve_model_spec(
        vendor,
        &config.model,
        u32::try_from(context_window).ok(),
        config.max_output_tokens,
        input_limit_for(vendor, config, plugin_config),
    )
}

/// The most input tokens one request may send to the model `config` names,
/// from the row that lists it.
///
/// A model rig's built-in catalog files keeps the catalog's fact: the cache row
/// is not consulted for it, so an upstream `max_input_tokens` (or its absence)
/// stands. Any other model reads `limit.input` from `plugin_config`'s models
/// cache row for the config's own cache section, `None` when the row does not
/// list it or no cache is loaded.
fn input_limit_for(
    vendor: &str,
    config: &Config,
    plugin_config: Option<&PluginConfig>,
) -> Option<u32> {
    let id = ProviderId::new(vendor, Format::OpenAi).or_else(|| ProviderId::catalog(vendor))?;
    if let Some(builtin) = Catalog::builtin().get(id, &config.model) {
        return builtin.spec.max_input_tokens;
    }
    plugin_config?
        .models_cache
        .as_ref()?
        .get_spec(config.cache_section(), &config.model)?
        .limit
        .input
}

/// A compactor that summarizes a conversation's older messages with the LLM and
/// splices the summary in through rig-memory.
///
/// The model is held as `Arc<Mutex<DynModel<Completion>>>` so it can be swapped
/// at runtime via `set_model()` — `DynModel` is rig's erased, cloneable model
/// handle, so the concrete type stays fixed regardless of provider. The model is
/// constructed eagerly at startup and never empty; the runtime shares its own
/// cell, so a model rebuilt after a switch reaches `compact()` as well.
///
/// The planning spec is held the same way, through `set_spec()`: it follows the
/// active model, so a switched-to model is planned against its own window. A
/// `None` spec means the configured provider resolved to no rig provider id:
/// compaction is disabled for the session, and `compact()` emits
/// `CompactionRecord::Failed` and returns `Err` rather than reporting a silent
/// success.
pub struct NuCompactor {
    model: Arc<Mutex<DynModel<Completion>>>,
    bus: Bus,
    policy: CompactionPolicy,
    spec: Arc<Mutex<Option<ModelSpec>>>,
}

impl Clone for NuCompactor {
    fn clone(&self) -> Self {
        Self {
            model: Arc::clone(&self.model),
            bus: self.bus.clone(),
            policy: self.policy.clone(),
            spec: Arc::clone(&self.spec),
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
            spec: Arc::new(Mutex::new(spec)),
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
            spec: Arc::new(Mutex::new(spec)),
        }
    }

    /// Swap the model used by subsequent `compact()` calls.
    ///
    /// Locks the shared inner model and replaces it. Any in-flight `compact()`
    /// call keeps the model clone it already took, so it finishes with the old
    /// model; the next call uses the new one.
    pub fn set_model(&self, model: DynModel<Completion>) {
        *self.lock_model() = model;
    }

    /// Set the planning spec `compact()` sizes its summary against.
    ///
    /// The spec follows the active model: the caller re-resolves it whenever it
    /// builds a model ([`ModelSlot::ensure`](super::super::state::provider::ModelSlot)),
    /// so a switched-to model is planned against its own window instead of the
    /// one the session started on. `None` disables compaction for the session.
    pub fn set_spec(&self, spec: Option<ModelSpec>) {
        *self.lock_spec() = spec;
    }

    /// The policy this compactor plans with.
    pub fn policy(&self) -> &CompactionPolicy {
        &self.policy
    }

    /// The spec this compactor plans against, `None` when compaction is
    /// disabled for the session.
    pub fn spec(&self) -> Option<ModelSpec> {
        self.lock_spec().clone()
    }

    /// The model this compactor summarizes with, cloned from the shared slot.
    ///
    /// Not the spec [`Self::spec`] returns: that one is built from our config
    /// and carries no pricing. This model's own `spec()` is the catalog-bound
    /// one, priced.
    pub fn model(&self) -> DynModel<Completion> {
        self.lock_model().clone()
    }

    /// The shared model slot, recovered if a swap panicked mid-write.
    fn lock_model(&self) -> MutexGuard<'_, DynModel<Completion>> {
        self.model.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The shared planning spec, recovered if a swap panicked mid-write.
    fn lock_spec(&self) -> MutexGuard<'_, Option<ModelSpec>> {
        self.spec.lock().unwrap_or_else(PoisonError::into_inner)
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
    /// as `CompactionRecord::Failed`.
    pub async fn compact(
        &self,
        store: std::sync::Arc<dyn rig_memory::Store>,
        conversation_id: &str,
        reason: &CompactReason,
        source: CompactionSource,
    ) -> Result<Option<CompactionOutcome>, MemoryError> {
        let spec = self.spec();
        let Some(spec) = spec.as_ref() else {
            let message =
                "Compaction is disabled: the session's provider resolved to no rig provider id"
                    .to_string();
            self.emit_failed(source, &message).await;
            return Err(MemoryError::backend(message));
        };

        let loaded = crate::session::blocking::on_pool({
            let store = std::sync::Arc::clone(&store);
            let log = conversation_id.to_owned();
            move || rig_memory::load(&*store, &log)
        })
        .await;
        let mut conversation = match loaded {
            Ok(Ok(conversation)) => conversation,
            Ok(Err(e)) => {
                let message = format!("Compaction load error: {e}");
                self.emit_failed(source, &message).await;
                return Err(MemoryError::backend(message));
            }
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
            conversation = self
                .save(
                    std::sync::Arc::clone(&store),
                    conversation_id.to_owned(),
                    conversation,
                    source,
                )
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
        self.emit_started(source).await;

        let summary = self.stream_summary(&plan, source).await?;

        let mut next_state = plan.state;
        next_state.summary = summary;
        if rig_memory::apply_summary(&mut conversation, plan.keep_from, &next_state).is_none() {
            let message = "Compaction summary could not be spliced in".to_string();
            self.emit_failed(source, &message).await;
            return Err(MemoryError::backend(message));
        }
        conversation = self
            .save(
                std::sync::Arc::clone(&store),
                conversation_id.to_owned(),
                conversation,
                source,
            )
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
        source: CompactionSource,
    ) -> Result<String, MemoryError> {
        // Lock the shared model and clone it so the LLM call runs against a
        // stable model; a concurrent `set_model()` then only affects the next
        // call.
        let model = self.model();
        engine::stream_text(
            self.wires(),
            &model,
            plan.request.clone(),
            source,
            Chunks::Publish,
        )
        .await
    }

    /// Save `conversation` to `store`, answering it back and emitting `Failed`
    /// when the write fails.
    async fn save(
        &self,
        store: std::sync::Arc<dyn rig_memory::Store>,
        conversation_id: String,
        mut conversation: rig::conversation::Conversation,
        source: CompactionSource,
    ) -> Result<rig::conversation::Conversation, MemoryError> {
        let saved = crate::session::blocking::on_pool(move || {
            let saved = rig_memory::save(&*store, &conversation_id, &mut conversation);
            (saved, conversation)
        })
        .await;
        match saved {
            Ok((Ok(()), conversation)) => Ok(conversation),
            Ok((Err(e), _)) => {
                let message = format!("Compaction save error: {e}");
                self.emit_failed(source, &message).await;
                Err(MemoryError::backend(message))
            }
            Err(e) => {
                let message = format!("Compaction save error: {e}");
                self.emit_failed(source, &message).await;
                Err(MemoryError::backend(message))
            }
        }
    }

    /// Run one self-contained compaction call: stream `request` to `model` and
    /// return its validated text.
    ///
    /// The manual path sends its plan's request; a hook-side side-call sends a
    /// request it planned itself. `chunks` decides whether the deltas reach the
    /// surfaces: a summary streams, notes do not.
    ///
    /// # Errors
    ///
    /// When the stream cannot be opened, fails, is cancelled, or answers text
    /// that cannot be compaction text. Every failure is also emitted as
    /// `CompactionRecord::Failed`.
    pub(crate) async fn complete(
        &self,
        model: &DynModel<Completion>,
        request: CompletionRequest,
        source: CompactionSource,
        chunks: Chunks,
    ) -> Result<String, MemoryError> {
        engine::stream_text(self.wires(), model, request, source, chunks).await
    }

    /// Emit the compaction pipeline's `Started` rows.
    pub(crate) async fn emit_started(&self, source: CompactionSource) {
        engine::emit_started(self.wires(), source).await;
    }

    /// Emit `CompactionRecord::Failed` carrying `message`.
    pub(crate) async fn emit_failed(&self, source: CompactionSource, message: &str) {
        engine::emit_failed(self.wires(), source, message).await;
    }

    /// Emit the compaction pipeline's `Completed` rows for `summary`.
    async fn emit_completed(&self, summary: &str, source: CompactionSource) {
        engine::emit_completed(self.wires(), summary, source).await;
    }

    /// The channels this compactor publishes on, taken one by one from the hub
    /// it was built with.
    fn wires(&self) -> engine::Wires<'_> {
        engine::Wires {
            stream: self.bus.stream(),
            narration: self.bus.narration(),
            lifecycle: self.bus.lifecycle(),
            cancel: self.bus.cancel(),
        }
    }
}
