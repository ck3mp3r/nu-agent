use crate::agent::mailbox::IncomingMessage;
use crate::agent::protocol::persona::PersonaSummary;
use crate::config::AgentsConfig;

pub(crate) struct MultiAgentState {
    mailbox_rx: Option<std::sync::mpsc::Receiver<IncomingMessage>>,
    available_agent_summaries: Vec<PersonaSummary>,
    agents_config: AgentsConfig,
}

impl MultiAgentState {
    pub(crate) fn new(
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

    pub(crate) fn take_mailbox_rx(&mut self) -> Option<std::sync::mpsc::Receiver<IncomingMessage>> {
        self.mailbox_rx.take()
    }

    pub(crate) fn available_agent_summaries(&self) -> &[PersonaSummary] {
        &self.available_agent_summaries
    }

    pub(crate) fn agents_config(&self) -> &AgentsConfig {
        &self.agents_config
    }
}
