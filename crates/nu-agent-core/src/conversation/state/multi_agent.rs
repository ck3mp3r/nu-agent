use super::super::managers::MultiAgentManager;
use crate::config::AgentsConfig;
use crate::mailbox::{AgentMailbox, IncomingMessage};
use crate::protocol::persona::PersonaSummary;

pub struct MultiAgentState {
    /// Bound socket for this agent. Kept alive here so the socket file and
    /// accept loop live for the full duration of the agent session. Dropped
    /// when the runtime is dropped, which cancels the accept loop and removes
    /// the socket file.
    mailbox: Option<AgentMailbox>,
    mailbox_rx: Option<std::sync::mpsc::Receiver<IncomingMessage>>,
    available_agent_summaries: Vec<PersonaSummary>,
    agents_config: AgentsConfig,
}

impl MultiAgentState {
    pub fn new(
        mailbox: Option<AgentMailbox>,
        mailbox_rx: Option<std::sync::mpsc::Receiver<IncomingMessage>>,
        available_agent_summaries: Vec<PersonaSummary>,
        agents_config: AgentsConfig,
    ) -> Self {
        Self {
            mailbox,
            mailbox_rx,
            available_agent_summaries,
            agents_config,
        }
    }

    pub fn socket_path(&self) -> Option<&std::path::Path> {
        self.mailbox.as_ref().map(|m| m.socket_path())
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
