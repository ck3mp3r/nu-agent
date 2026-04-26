mod entry;
pub use entry::{AuditEntry, AuditError, AuditResult};

use std::path::{Path, PathBuf};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

pub struct AuditLogger {
    log_path: PathBuf,
}

impl AuditLogger {
    /// Create a new AuditLogger at ~/.local/share/nu-agent/tool_audit.log
    pub fn new() -> Result<Self, AuditError> {
        let log_dir = dirs::data_local_dir()
            .ok_or_else(|| AuditError::Write("Could not determine data directory".to_string()))?
            .join("nu-agent");

        std::fs::create_dir_all(&log_dir)?;
        let log_path = log_dir.join("tool_audit.log");

        Ok(Self { log_path })
    }

    /// For testing: create logger with custom path
    #[cfg(test)]
    pub(crate) fn with_path(log_path: PathBuf) -> Self {
        Self { log_path }
    }

    /// Append an audit entry to the log file
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
mod tests;
