use super::super::managers::MultiAgentManager;
use crate::config::AgentsConfig;
use crate::mailbox::IncomingMessage;
use crate::protocol::persona::PersonaSummary;

pub struct MultiAgentState {
    mailbox_rx: Option<std::sync::mpsc::Receiver<IncomingMessage>>,
    available_agent_summaries: Vec<PersonaSummary>,
    agents_config: AgentsConfig,
}

impl MultiAgentState {
    pub fn new(
        mailbox_rx: Option<std::sync::mpsc::Receiver<IncomingMessage>>,
        available_agent_summaries: Vec<PersonaSummary>,
        agents_config: AgentsConfig,
    ) -> Self {
        Self {
            mailbox_rx,
            available_agent_summaries,
            agents_config,
        }
    }

    pub fn take_mailbox_rx(&mut self) -> Option<std::sync::mpsc::Receiver<IncomingMessage>> {
        self.mailbox_rx.take()
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

    fn take_mailbox_rx(&mut self) -> Option<std::sync::mpsc::Receiver<IncomingMessage>> {
        self.mailbox_rx.take()
    }
}
