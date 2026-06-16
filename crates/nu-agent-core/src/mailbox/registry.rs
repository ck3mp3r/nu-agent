use std::collections::HashMap;

use super::protocol::ServerFrame;

pub struct AgentRegistry {
    pending: HashMap<String, String>,
    connected: HashMap<String, ConnectedAgent>,
}

struct ConnectedAgent {
    writer: tokio::sync::mpsc::Sender<ServerFrame>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            connected: HashMap::new(),
        }
    }

    pub fn register_pending(&mut self, token: String, name: String) {
        self.pending.insert(token, name);
    }

    pub fn authenticate(&mut self, token: &str) -> Option<String> {
        self.pending.remove(token)
    }

    pub fn add_connected(&mut self, name: String, sender: tokio::sync::mpsc::Sender<ServerFrame>) {
        self.connected
            .insert(name, ConnectedAgent { writer: sender });
    }

    pub fn remove_connected(&mut self, name: &str) {
        self.connected.remove(name);
    }

    pub fn connected_names(&self) -> Vec<String> {
        self.connected.keys().cloned().collect()
    }

    #[cfg(test)]
    pub fn is_connected(&self, name: &str) -> bool {
        self.connected.contains_key(name)
    }

    pub fn route_message(&self, to: &str, frame: ServerFrame) -> Result<(), String> {
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
