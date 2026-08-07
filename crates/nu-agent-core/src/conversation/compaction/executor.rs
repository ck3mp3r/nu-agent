//! Owns the logic for executing a compaction event.
//! Extracted from AgentConversationRuntime::execute_compaction_event to give it a single
//! responsibility. AgentConversationRuntime constructs a CompactionExecutor and delegates.

use super::invocation::{
    COMPACTION_FAILURE_WARNING, CompactionInvocation, execute_compaction_event_shared,
    execute_compaction_with_config,
};
use crate::compaction::CompactionParams;
use crate::config::Config;
use crate::conversation::providers::{CachedProviderClient, ModelVisitor};
use crate::protocol::compaction::CompactionTriggerSource;
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use crate::session::{CachedMemory, SessionStore};

pub struct CompactionExecutor<'a, S: SessionStore + Clone + Send + Sync> {
    config: &'a Config,
    memory: &'a CachedMemory<S>,
    final_session_id: &'a str,
    compaction_params: CompactionParams,
}

impl<'a, S: SessionStore + Clone + Send + Sync> CompactionExecutor<'a, S> {
    pub fn new(
        config: &'a Config,
        memory: &'a CachedMemory<S>,
        final_session_id: &'a str,
        compaction_params: CompactionParams,
    ) -> Self {
        Self {
            config,
            memory,
            final_session_id,
            compaction_params,
        }
    }

    /// Execute compaction. Returns `Ok(Some(summary_total_tokens))` when
    /// compaction was triggered, or `Ok(None)` when compaction was skipped.
    /// Caller is responsible for setting `last_total_tokens` to
    /// `summary_total_tokens` when `Some` is returned.
    /// Returns `Err(message)` on failure.
    pub async fn execute<U: ProgressUi + Send>(
        &self,
        ui: &mut U,
        source: CompactionTriggerSource,
        cached_client: &CachedProviderClient,
    ) -> Result<Option<Option<u64>>, String> {
        let source_label = source.as_str().to_string();

        // Visitor that executes compaction with the provider's completion model.
        struct CompactionVisitor<'a, S, U>
        where
            S: SessionStore + Clone + Send + Sync,
        {
            source: CompactionTriggerSource,
            memory: &'a CachedMemory<S>,
            session_id: &'a str,
            compaction_params: CompactionParams,
            ui: &'a mut U,
            source_label: &'a str,
        }

        impl<S, U> ModelVisitor for CompactionVisitor<'_, S, U>
        where
            S: SessionStore + Clone + Send + Sync,
            S::Error: std::fmt::Display,
            U: ProgressUi + Send,
        {
            type Output = Result<Option<(UiEvent, Option<u64>)>, String>;

            async fn visit<M>(self, model: M) -> Self::Output
            where
                M: rig::completion::CompletionModel + Clone + 'static,
            {
                let session_id = self.session_id.to_string();
                let compaction_params = self.compaction_params.clone();
                let memory = self.memory.clone();
                let source_label = self.source_label.to_string();
                let source = self.source;
                execute_compaction_event_shared(source, move || {
                    let session_id = session_id.clone();
                    let compaction_params = compaction_params.clone();
                    let memory = memory.clone();
                    let model = model.clone();
                    let source_label = source_label.clone();
                    async move {
                        execute_compaction_with_config::<S, M, _>(
                            &session_id,
                            &compaction_params,
                            &memory,
                            model,
                            self.ui,
                            CompactionInvocation {
                                source: &source_label,
                            },
                        )
                        .await
                    }
                })
                .await
            }
        }

        ui.emit(&UiEvent::CompactionStarted {
            source: source_label.clone(),
        });

        let result = cached_client
            .with_model(
                &self.config.model,
                CompactionVisitor {
                    source,
                    memory: self.memory,
                    session_id: self.final_session_id,
                    compaction_params: self.compaction_params.clone(),
                    ui,
                    source_label: &source_label,
                },
            )
            .await;

        match result {
            Ok(Some((event, summary_total_tokens))) => {
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
