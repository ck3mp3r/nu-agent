use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("Failed to create audit log directory: {0}")]
    CreateDir(#[from] std::io::Error),

    #[error("Failed to serialize audit entry: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("Failed to write audit log: {0}")]
    Write(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub result: AuditResult,
    pub duration_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AuditResult {
    Ok(serde_json::Value),
    Err(String),
}
