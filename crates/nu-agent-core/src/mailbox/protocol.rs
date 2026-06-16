use serde::{Deserialize, Serialize};

fn default_kind() -> String {
    "message".to_string()
}

/// Incoming message from broker for agent runtime
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    pub from: String,
    pub message: String,
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientFrame {
    #[serde(rename = "auth")]
    Auth { token: String },
    #[serde(rename = "message")]
    Message {
        to: String,
        message: String,
        #[serde(default = "default_kind")]
        kind: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerFrame {
    #[serde(rename = "auth_ok")]
    AuthOk { name: String },
    #[serde(rename = "auth_rejected")]
    AuthRejected { reason: String },
    #[serde(rename = "message")]
    Message {
        from: String,
        message: String,
        #[serde(default = "default_kind")]
        kind: String,
    },
}
