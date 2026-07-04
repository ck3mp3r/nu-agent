/// Runtime capability for model and agent switching.
pub trait HasModelSwitching {
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String>;

    fn switch_agent(&mut self, agent_name: &str) -> Result<String, String>;

    fn active_model_identity(&self) -> String;

    fn max_context_tokens(&self) -> Option<u64>;
}
