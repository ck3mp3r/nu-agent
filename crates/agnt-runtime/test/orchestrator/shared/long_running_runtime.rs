use super::*;

pub(crate) struct LongRunningRuntime {
    pub(crate) prompts: Arc<Mutex<Vec<String>>>,
    pub(crate) switched_models: Arc<Mutex<Vec<String>>>,
    /// Every agent switch this runtime was asked to apply, in order.
    pub(crate) switched_agents: Arc<Mutex<Vec<String>>>,
    pub(crate) action_log: Arc<Mutex<Vec<String>>>,
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) block_first_turn: Arc<AtomicBool>,
    pub(crate) switch_model_result: Option<Result<String, String>>,
    pub(crate) active_identity: Arc<Mutex<String>>,
    pub(crate) bus: crate::bus::Bus,
    pub(crate) turn_started_tx: tokio::sync::watch::Sender<bool>,
}

impl LongRunningRuntime {
    pub(crate) fn new(block_first_turn: Arc<AtomicBool>) -> Self {
        let (turn_started_tx, _) = tokio::sync::watch::channel(false);
        Self {
            prompts: Arc::new(Mutex::new(Vec::new())),
            switched_models: Arc::new(Mutex::new(Vec::new())),
            switched_agents: Arc::new(Mutex::new(Vec::new())),
            action_log: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(AtomicBool::new(false)),
            block_first_turn,
            switch_model_result: None,
            active_identity: Arc::new(Mutex::new("openai/gpt-4o-mini".to_string())),
            bus: crate::bus::Bus::default(),
            turn_started_tx,
        }
    }

    /// Signals when the blocking `first` turn has begun. Tests await this
    /// instead of sleeping before they unblock the turn.
    pub(crate) fn turn_started_rx(&self) -> tokio::sync::watch::Receiver<bool> {
        self.turn_started_tx.subscribe()
    }

    pub(crate) fn with_switch_model_result(mut self, result: Result<String, String>) -> Self {
        self.switch_model_result = Some(result);
        self
    }

    pub(crate) fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        self.bus = bus;
        self
    }
}

impl CoreRuntime for LongRunningRuntime {
    async fn execute_turn(
        &mut self,
        prompt: String,
        _context: Option<String>,
        span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        self.active.store(true, Ordering::SeqCst);
        self.action_log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("turn:{prompt}"));
        self.prompts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(prompt.clone());

        if prompt == "first" {
            // Signal that the blocking turn has begun so tests can unblock it
            // without sleeping.
            let _ = self.turn_started_tx.send(true);
            let mut cancel_rx = self.bus.cancel().subscribe();
            while !self.block_first_turn.load(Ordering::SeqCst) {
                if matches!(
                    cancel_rx.try_recv(),
                    Ok(crate::bus::CancelEvent::Requested)
                        | Err(crate::bus::TryRecvError::Lagged(_))
                ) {
                    self.active.store(false, Ordering::SeqCst);
                    return TurnEnd::Cancelled {
                        value: Value::nothing(span),
                    };
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }

        // Publish the turn spine directly to the bus (the worker bridge no
        // longer converts events), matching production.
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
            task_id: None,
            duration_ms: 0,
        });
        self.active.store(false, Ordering::SeqCst);
        TurnEnd::Completed {
            value: Value::nothing(span),
        }
    }
}

impl ModelSwitching for LongRunningRuntime {
    async fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.action_log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("switch:{model_spec}"));
        self.switched_models
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(model_spec.to_string());

        if let Some(result) = self.switch_model_result.clone() {
            return result.map(|identity| (identity, None));
        }

        let mut identity = self
            .active_identity
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *identity = model_spec.to_string();
        Ok((model_spec.to_string(), None))
    }

    async fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
        self.switched_agents
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(agent_name.to_string());
        Ok(agent_name.to_string())
    }

    fn active_model_identity(&self) -> String {
        self.active_identity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

crate::default_session!(LongRunningRuntime);
crate::default_mcp!(LongRunningRuntime);
