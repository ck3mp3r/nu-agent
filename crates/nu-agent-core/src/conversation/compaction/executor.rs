//! Owns the logic for executing a compaction event.
//! Extracted from AgentConversationRuntime::execute_compaction_event to give it a single
//! responsibility. AgentConversationRuntime constructs a CompactionExecutor and delegates.

use super::invocation::{
    COMPACTION_FAILURE_WARNING, CompactionInvocation, execute_compaction,
    execute_compaction_event_shared,
};
use crate::config::Config;
use crate::conversation::providers::{CachedProviderClient, ModelVisitor};
use crate::protocol::compaction::CompactionTriggerSource;
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use crate::session::{JournalConversationMemory, SessionStore};

pub struct CompactionExecutor<'a> {
    config: &'a Config,
    runtime: &'a tokio::runtime::Runtime,
    memory: &'a JournalConversationMemory,
    store: &'a SessionStore,
    final_session_id: &'a str,
}

impl<'a> CompactionExecutor<'a> {
    pub fn new(
        config: &'a Config,
        runtime: &'a tokio::runtime::Runtime,
        memory: &'a JournalConversationMemory,
        store: &'a SessionStore,
        final_session_id: &'a str,
    ) -> Self {
        Self {
            config,
            runtime,
            memory,
            store,
            final_session_id,
        }
    }

    #[cfg(test)]
    pub fn session_id(&self) -> &str {
        self.final_session_id
    }

    /// Execute compaction. Returns `Ok(Some(summary_total_tokens))` when
    /// compaction was triggered, or `Ok(None)` when compaction was skipped.
    /// Caller is responsible for setting `last_total_tokens` to
    /// `summary_total_tokens` when `Some` is returned.
    /// Returns `Err(message)` on failure.
    pub fn execute<U: ProgressUi>(
        &self,
        ui: &mut U,
        source: CompactionTriggerSource,
        cached_client: &CachedProviderClient,
    ) -> Result<Option<Option<u64>>, String> {
        let source_label = source.as_str().to_string();

        // Load session temporarily for compaction
        let mut session = self
            .store
            .load_session(self.final_session_id)
            .map_err(|e| format!("Failed to load session for compaction: {}", e))?;

        // Visitor that executes compaction with the provider's completion model.
        struct CompactionVisitor<'a, U> {
            source: CompactionTriggerSource,
            runtime: &'a tokio::runtime::Runtime,
            memory: &'a JournalConversationMemory,
            session: &'a mut crate::session::Session,
            ui: &'a mut U,
            source_label: &'a str,
        }

        impl<U: ProgressUi> ModelVisitor for CompactionVisitor<'_, U> {
            type Output = Result<Option<(UiEvent, Option<u64>)>, String>;

            fn visit<M>(self, model: M) -> Self::Output
            where
                M: rig::completion::CompletionModel + Clone + 'static,
            {
                execute_compaction_event_shared(self.source, || {
                    self.runtime.block_on(execute_compaction(
                        self.session,
                        self.memory,
                        model.clone(),
                        self.ui,
                        CompactionInvocation {
                            source: self.source_label,
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
                session: &mut session,
                ui,
                source_label: &source_label,
            },
        );

        match result {
            Ok(Some((event, summary_total_tokens))) => {
                ui.emit(&UiEvent::CompactionStarted {
                    source: source_label,
                });
                ui.emit(&event);
                Ok(Some(summary_total_tokens))
            }
            Ok(None) => Ok(None),
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
#[path = "executor_test.rs"]
mod executor_test;
