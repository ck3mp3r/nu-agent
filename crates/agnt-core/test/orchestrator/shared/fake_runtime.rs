use super::*;

#[derive(Default)]
pub(crate) struct FakeRuntime {
    pub(crate) prompts: Vec<String>,
    pub(crate) run_compaction_calls: usize,
    pub(crate) run_compaction_sources: Vec<String>,
    pub(crate) run_compaction_fail: bool,
    pub(crate) switched_models: Vec<String>,
    pub(crate) switch_model_result: Option<Result<String, String>>,
    pub(crate) session_id: Option<String>,
    pub(crate) bus: crate::bus::Bus,
}

impl CoreRuntime for FakeRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        // The worker bridge no longer converts `SystemEvent::TurnCompleted` to a
        // `TurnEvent::Completed` on the bus. Publish it directly, matching
        // production (see executor.rs).
        let _ = self
            .bus
            .ui_event()
            .send(SystemEvent::TurnCompleted {
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: None,
                cost: None,
                cost_complete: true,
                duration_ms: 0,
            })
            .await;
        Ok(Value::nothing(Span::test_data()))
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

impl SessionState for FakeRuntime {}

impl SessionPersistence for FakeRuntime {
    fn current_session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    async fn run_compaction(&mut self, source: &str) -> Result<(), String> {
        self.run_compaction_calls = self.run_compaction_calls.saturating_add(1);
        self.run_compaction_sources.push(source.to_string());
        if self.run_compaction_fail {
            return Err("auto compaction failed".to_string());
        }
        // Publish lifecycle events directly to the bus so test-harness quit-gates
        // that subscribe to bus.compaction() fire (matching production, where the
        // compactor publishes the compaction facts on the bus directly).
        let _ = self
            .bus
            .compaction()
            .send(SystemEvent::CompactionCompleted {
                source: source.to_string(),
                summary_preview: "summary".to_string(),
                summary_body: "summary".to_string(),
            })
            .await;
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct FakeValueRuntime {
    pub(crate) prompts: Vec<String>,
    pub(crate) bus: crate::bus::Bus,
}

impl CoreRuntime for FakeValueRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        // Publish TurnCompleted directly to the bus (worker bridge no longer
        // converts SystemEvent::TurnCompleted), matching production.
        let _ = self
            .bus
            .ui_event()
            .send(SystemEvent::TurnCompleted {
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: None,
                cost: None,
                cost_complete: true,
                duration_ms: 0,
            })
            .await;
        Ok(Value::record(nu_protocol::Record::new(), span))
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
