/// Holds deferred model and agent switch requests that arrived while the worker
/// was busy executing a turn. The orchestrator drains these via
/// [`take_queued_model_switch`] / [`take_queued_agent_switch`] once the worker
/// becomes idle.
pub struct PendingOps {
    queued_model_switch: Option<String>,
    queued_agent_switch: Option<String>,
}

impl Default for PendingOps {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingOps {
    pub fn new() -> Self {
        Self {
            queued_model_switch: None,
            queued_agent_switch: None,
        }
    }

    pub fn has_pending(&self) -> bool {
        self.queued_model_switch.is_some() || self.queued_agent_switch.is_some()
    }

    pub fn queue_model_switch(&mut self, model_spec: String) {
        self.queued_model_switch = Some(model_spec);
    }

    pub fn queue_agent_switch(&mut self, agent_name: String) {
        self.queued_agent_switch = Some(agent_name);
    }

    pub fn take_queued_model_switch(&mut self) -> Option<String> {
        self.queued_model_switch.take()
    }

    pub fn take_queued_agent_switch(&mut self) -> Option<String> {
        self.queued_agent_switch.take()
    }
}
