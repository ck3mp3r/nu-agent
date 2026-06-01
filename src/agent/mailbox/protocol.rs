use serde::{Deserialize, Serialize};

/// Incoming message from broker for agent runtime
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct IncomingMessage {
    pub from: String,
    pub message: String,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ClientFrame {
    #[serde(rename = "auth")]
    Auth { token: String },
    #[serde(rename = "message")]
    Message { to: String, message: String },
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ServerFrame {
    #[serde(rename = "auth_ok")]
    AuthOk { name: String },
    #[serde(rename = "auth_rejected")]
    AuthRejected { reason: String },
    #[serde(rename = "message")]
    Message { from: String, message: String },
}
