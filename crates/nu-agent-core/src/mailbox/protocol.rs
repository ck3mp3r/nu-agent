use serde::{Deserialize, Serialize};

fn default_kind() -> String {
    "message".to_string()
}

/// Delivered to the agent conversation loop via std::sync::mpsc.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    pub from: String,
    pub message: String,
    pub kind: String,
}

/// Wire format — one JSON line per connection.
#[derive(Debug, Serialize, Deserialize)]
pub struct MessageFrame {
    pub from: String,
    pub message: String,
    #[serde(default = "default_kind")]
    pub kind: String,
}
