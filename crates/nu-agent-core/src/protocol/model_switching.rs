/// Runtime capability for model and agent switching.
pub trait ModelSwitching {
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String>;

    fn switch_agent(&mut self, agent_name: &str) -> Result<String, String>;

    fn active_model_identity(&self) -> String;

    fn max_context_tokens(&self) -> Option<u64>;

    /// The current agent's description, if any.
    ///
    /// Defaults to `None` for runtimes that do not track agent descriptions.
    fn agent_description(&self) -> Option<&str> {
        None
    }

    /// The current agent's icon, if any.
    ///
    /// Defaults to `None` for runtimes that do not track agent icons.
    fn agent_icon(&self) -> Option<&str> {
        None
    }
}
