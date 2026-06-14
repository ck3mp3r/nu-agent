//! Owns the logic for executing a compaction event.
//! Extracted from AgentConversationRuntime::execute_compaction_event to give it a single
//! responsibility. AgentConversationRuntime constructs a CompactionExecutor and delegates.

use crate::agent::conversation::compaction::{
    COMPACTION_FAILURE_WARNING, CompactionInvocation, execute_compaction,
    execute_compaction_event_shared,
};
use crate::agent::conversation::providers::{CachedProviderClient, ModelVisitor};
use crate::agent::protocol::compaction::CompactionTriggerSource;
use crate::agent::protocol::contracts::ProgressUi;
use crate::agent::protocol::event::UiEvent;
use crate::compaction::CompactionInvocationMode;
use crate::config::Config;
use crate::session::{JsonlConversationStore, SessionStore};
use crate::types::InMemoryConversationMemory;

pub(crate) struct CompactionExecutor<'a> {
    pub(crate) config: &'a Config,
    pub(crate) runtime: &'a tokio::runtime::Runtime,
    pub(crate) memory: &'a InMemoryConversationMemory,
    pub(crate) conversation_store: &'a JsonlConversationStore,
    pub(crate) store: &'a SessionStore,
    pub(crate) last_total_tokens: Option<u64>,
    pub(crate) final_session_id: &'a str,
}

impl<'a> CompactionExecutor<'a> {
    pub(crate) fn new(
        config: &'a Config,
        runtime: &'a tokio::runtime::Runtime,
        memory: &'a InMemoryConversationMemory,
        conversation_store: &'a JsonlConversationStore,
        store: &'a SessionStore,
        last_total_tokens: Option<u64>,
        final_session_id: &'a str,
    ) -> Self {
        Self {
            config,
            runtime,
            memory,
            conversation_store,
            store,
            last_total_tokens,
            final_session_id,
        }
    }

    /// Execute compaction. Returns `Ok(Some(new_compaction_count))` when compaction
    /// was triggered, or `Ok(None)` when compaction was skipped.
    /// Caller is responsible for updating `compaction_count` and resetting
    /// `last_total_tokens` to `None` when `Some` is returned.
    /// Returns `Err(message)` on failure.
    pub(crate) fn execute<U: ProgressUi>(
        &self,
        ui: &mut U,
        source: CompactionTriggerSource,
        cached_client: &CachedProviderClient,
    ) -> Result<Option<usize>, String> {
        let source_label = source.as_str().to_string();
        ui.emit(&UiEvent::CompactionStarted {
            source: source_label.clone(),
        });

        // Load session temporarily for compaction
        let mut session = self
            .store
            .load_session(self.final_session_id)
            .map_err(|e| format!("Failed to load session for compaction: {}", e))?;

        // Visitor that executes compaction with the provider's completion model.
        struct CompactionVisitor<'a, U> {
            source: CompactionTriggerSource,
            runtime: &'a tokio::runtime::Runtime,
            memory: &'a InMemoryConversationMemory,
            conversation_store: &'a JsonlConversationStore,
            session: &'a mut crate::session::Session,
            ui: &'a mut U,
            source_label: &'a str,
            last_total_tokens: Option<u64>,
        }

        impl<U: ProgressUi> ModelVisitor for CompactionVisitor<'_, U> {
            type Output = Result<UiEvent, String>;

            fn visit<M>(self, model: M) -> Self::Output
            where
                M: rig::completion::CompletionModel + Clone + 'static,
            {
                execute_compaction_event_shared(self.source, || {
                    let mode = match self.source {
                        CompactionTriggerSource::SlashCompact => CompactionInvocationMode::Force,
                        CompactionTriggerSource::AutoThreshold => {
                            CompactionInvocationMode::Threshold
                        }
                    };
                    self.runtime.block_on(execute_compaction(
                        self.session,
                        self.memory,
                        self.conversation_store,
                        model.clone(),
                        self.ui,
                        CompactionInvocation {
                            mode,
                            source: self.source_label,
                            last_total_tokens: self.last_total_tokens,
                        },
                    ))
                })
            }
        }

        let result = cached_client.with_model(
            &self.config.model,
            CompactionVisitor {
                source,
                runtime: self.runtime,
                memory: self.memory,
                conversation_store: self.conversation_store,
                session: &mut session,
                ui,
                source_label: &source_label,
                last_total_tokens: self.last_total_tokens,
            },
        );

        match result {
            Ok(event) => {
                ui.emit(&event);

                // Return new compaction count when compaction was triggered
                if let UiEvent::CompactionTriggered { .. } = &event {
                    Ok(Some(session.compaction_count()))
                } else {
                    // CompactionSkipped — no state change needed
                    Ok(None)
                }
            }
            Err(error) => {
                ui.emit(&UiEvent::CompactionFailed {
                    source: source_label,
                    message: COMPACTION_FAILURE_WARNING.to_string(),
                });
                Err(error)
            }
        }
    }
}

#[cfg(test)]
#[path = "compaction_executor_test.rs"]
mod compaction_executor_test;
