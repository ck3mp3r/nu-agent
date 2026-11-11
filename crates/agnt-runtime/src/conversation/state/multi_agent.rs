use super::super::managers::MultiAgentManager;
use crate::config::AgentsConfig;
use crate::protocol::persona::PersonaSummary;

pub struct MultiAgentState {
    available_agent_summaries: Vec<PersonaSummary>,
    agents_config: AgentsConfig,
}

impl MultiAgentState {
    pub fn new(
        available_agent_summaries: Vec<PersonaSummary>,
        agents_config: AgentsConfig,
    ) -> Self {
        Self {
            available_agent_summaries,
            agents_config,
        }
    }

    pub fn available_agent_summaries(&self) -> &[PersonaSummary] {
        &self.available_agent_summaries
    }

    pub fn agents_config(&self) -> &AgentsConfig {
        &self.agents_config
    }
}

impl MultiAgentManager for MultiAgentState {
    fn available_agent_summaries(&self) -> &[PersonaSummary] {
        &self.available_agent_summaries
    }

    fn agents_config(&self) -> &AgentsConfig {
        &self.agents_config
    }
}
