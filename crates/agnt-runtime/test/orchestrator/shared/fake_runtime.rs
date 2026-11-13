use agnt_kernel::CompactionSource;
use agnt_kernel::SessionId;

use super::*;

#[derive(Default)]
pub(crate) struct FakeRuntime {
    pub(crate) prompts: Vec<String>,
    pub(crate) run_compaction_calls: usize,
    pub(crate) run_compaction_sources: Vec<CompactionSource>,
    pub(crate) run_compaction_fail: bool,
    pub(crate) run_uncompact_calls: usize,
    pub(crate) switched_models: Vec<String>,
    pub(crate) switch_model_result: Option<Result<String, String>>,
    pub(crate) session_id: Option<String>,
    pub(crate) bus: crate::bus::Bus,
}

impl CoreRuntime for FakeRuntime {
    async fn execute_turn(
        &mut self,
        prompt: String,
        _context: Option<String>,
        span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        self.prompts.push(prompt.clone());
        // The worker bridge no longer converts events; the executor bracket owns
        // the turn spine, so the double publishes it directly, with the
        // attribution the dispatch handed it.
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnStarted {
            prompt,
            task_id: _task_id.clone(),
        });
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
            task_id: _task_id,
            duration_ms: 0,
        });
        TurnEnd::Completed {
            value: Value::nothing(span),
        }
    }
}

impl McpManagement for FakeRuntime {
    async fn set_mcp_server_enabled(
        &mut self,
        _name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        0
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }
}

impl ModelSwitching for FakeRuntime {
    async fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.switched_models.push(model_spec.to_string());
        if let Some(result) = self.switch_model_result.clone() {
            return result.map(|identity| (identity, None));
        }
        Ok((model_spec.to_string(), None))
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        "openai/gpt-4o-mini".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl SessionState for FakeRuntime {
    fn new_session(&mut self) {
        // The mint publishes its own record and spine row in production, and the
        // surface derives the transcript clear from that record. The double
        // publishes the same pair so a `/new` scenario observes the mint.
        self.session_id = Some("fake-new-session".to_string());
        let _ = self
            .bus
            .stream()
            .publish(SessionRecord::Session(SessionLifecycle::Started {
                session_id: SessionId::new("fake-new-session"),
                hydrated: false,
            }));
        let _ = self
            .bus
            .lifecycle()
            .publish(LifecycleEvent::SessionStarted {
                session_id: "fake-new-session".to_string(),
                hydrated: false,
            });
    }
}

impl SessionPersistence for FakeRuntime {
    fn current_session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    async fn run_compaction(&mut self, source: CompactionSource) -> Result<(), String> {
        self.run_compaction_calls = self.run_compaction_calls.saturating_add(1);
        self.run_compaction_sources.push(source);
        if self.run_compaction_fail {
            return Err("auto compaction failed".to_string());
        }
        // Publish the compaction records and spine directly to the bus so
        // test-harness quit-gates that read them fire (matching production,
        // where the compactor publishes on the bus directly).
        let _ = self
            .bus
            .stream()
            .publish(SessionRecord::Compaction(CompactionRecord::Completed {
                source,
                summary: "summary".to_string(),
            }));
        let _ = self
            .bus
            .lifecycle()
            .publish(LifecycleEvent::CompactionCompleted {
                source,
                summary: "summary".to_string(),
            });
        Ok(())
    }

    async fn uncompact(&mut self, source: CompactionSource) -> Result<(), String> {
        self.run_uncompact_calls = self.run_uncompact_calls.saturating_add(1);
        // The undo publishes its own terminal row, so the loop's active slot
        // clears exactly as it does in production.
        let _ =
            self.bus
                .stream()
                .publish(SessionRecord::Compaction(CompactionRecord::Uncompacted {
                    source,
                }));
        let _ = self
            .bus
            .lifecycle()
            .publish(LifecycleEvent::Uncompacted { source });
        Ok(())
    }
}

/// The picker reader defaults to an empty catalog: the fixture holds no persona
/// scan root and no session store.
impl crate::catalog::PickerCatalogSource for FakeRuntime {}

#[derive(Default)]
pub(crate) struct FakeValueRuntime {
    pub(crate) prompts: Vec<String>,
    pub(crate) bus: crate::bus::Bus,
}

impl CoreRuntime for FakeValueRuntime {
    async fn execute_turn(
        &mut self,
        prompt: String,
        _context: Option<String>,
        span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        self.prompts.push(prompt.clone());
        // The executor bracket owns the turn spine; the double publishes it
        // directly, carrying the attribution the dispatch handed it.
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnStarted {
            prompt,
            task_id: _task_id.clone(),
        });
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
            task_id: _task_id,
            duration_ms: 0,
        });
        TurnEnd::Completed {
            value: Value::record(nu_protocol::Record::new(), span),
        }
    }
}

impl ModelSwitching for FakeValueRuntime {
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        "unknown/unknown".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

crate::default_session!(FakeValueRuntime);
crate::default_mcp!(FakeValueRuntime);
