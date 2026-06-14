use std::collections::HashMap;

use super::protocol::ServerFrame;

pub(crate) struct AgentRegistry {
    pending: HashMap<String, String>,
    connected: HashMap<String, ConnectedAgent>,
}

struct ConnectedAgent {
    writer: tokio::sync::mpsc::Sender<ServerFrame>,
}

impl AgentRegistry {
    pub(crate) fn new() -> Self {
        Self {
            pending: HashMap::new(),
            connected: HashMap::new(),
        }
    }

    pub(crate) fn register_pending(&mut self, token: String, name: String) {
        self.pending.insert(token, name);
    }

    pub(crate) fn authenticate(&mut self, token: &str) -> Option<String> {
        self.pending.remove(token)
    }

    pub(crate) fn add_connected(
        &mut self,
        name: String,
        sender: tokio::sync::mpsc::Sender<ServerFrame>,
    ) {
        self.connected
            .insert(name, ConnectedAgent { writer: sender });
    }

    pub(crate) fn remove_connected(&mut self, name: &str) {
        self.connected.remove(name);
    }

    pub(crate) fn connected_names(&self) -> Vec<String> {
        self.connected.keys().cloned().collect()
    }

    #[cfg(test)]
    pub(crate) fn is_connected(&self, name: &str) -> bool {
        self.connected.contains_key(name)
    }

    pub(crate) fn route_message(&self, to: &str, frame: ServerFrame) -> Result<(), String> {
        match self.connected.get(to) {
            Some(agent) => {
                // Try to send, ignore if receiver dropped
                let _ = agent.writer.try_send(frame);
                Ok(())
            }
            None => Err(format!("Agent '{}' not connected", to)),
        }
    }
}
