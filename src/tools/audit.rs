use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use std::path::{Path, PathBuf};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

pub struct AuditLogger {
    log_path: PathBuf,
}

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

impl AuditLogger {
    /// Create a new AuditLogger with the given log file path
    pub fn new(log_path: PathBuf) -> Self {
        Self { log_path }
    }

    /// Append an audit entry to the log file.
    ///
    /// # IMPORTANT: Caller Responsibilities
    ///
    /// This method follows the Single Responsibility Principle and ONLY logs entries.
    /// **The caller MUST ensure the parent directory exists before calling this method.**
    ///
    /// ## Required Setup
    ///
    /// 1. Create the parent directory structure before instantiating AuditLogger
    /// 2. Pass the complete file path (including filename) to AuditLogger::new()
    /// 3. Only then call log() to write entries
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    /// use std::path::PathBuf;
    /// use nu_plugin_agent::tools::audit::{AuditLogger, AuditEntry, AuditResult};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Step 1: Define log path
    /// let log_dir = PathBuf::from("/var/log/app");
    /// let log_path = log_dir.join("audit.log");
    ///
    /// // Step 2: Create directory structure (caller responsibility)
    /// tokio::fs::create_dir_all(&log_dir).await?;
    ///
    /// // Step 3: Create logger with existing directory
    /// let logger = AuditLogger::new(log_path);
    ///
    /// // Step 4: Now logging will succeed
    /// let entry = AuditEntry {
    ///     timestamp: chrono::Utc::now(),
    ///     tool_name: "my_tool".to_string(),
    ///     args: serde_json::json!({"key": "value"}),
    ///     result: AuditResult::Ok(serde_json::json!("success")),
    ///     duration_ms: 42,
    /// };
    /// logger.log(entry).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The log file cannot be opened or created
    /// - **The parent directory does not exist** (filesystem I/O error)
    /// - Writing to the file fails
    /// - Syncing the file to disk fails
    pub async fn log(&self, entry: AuditEntry) -> Result<(), AuditError> {
        let mut json_line = serde_json::to_string(&entry)?;
        json_line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .await?;

        file.write_all(json_line.as_bytes()).await?;
        file.sync_all().await?;

        Ok(())
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod audit_tests;
